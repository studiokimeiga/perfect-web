# perfect-web developer commands.
# PROJECT_CHARTER.md §13.4 defines the stable command surface. Recipes are added
# as milestones make them applicable; unimplemented ones fail loudly with the
# milestone that will provide them, rather than silently succeeding.

# A report filter must not turn a failed producer into a successful gate.
# errexit also stops grouped evidence commands before later summaries run.
set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments

toolchain_bin := justfile_directory() / ".toolchain/prefix/bin"
export PATH := toolchain_bin + ":" + env_var('PATH')

# The parallel tracks' recipes (ADR-0253, docs/PARALLEL.md): each track adds
# recipes only to its own file, and the integrator alone edits this one. An
# imported recipe runs in this directory, under these settings.
import 'just/identity.just'
import 'just/uploads.just'
import 'just/upload-metadata.just'
import 'just/notifications.just'
import 'just/messages.just'
import 'just/kiokun.just'
import 'just/kiokun-test-data.just'
import 'just/store-accounts.just'
import 'just/store-search.just'
import 'just/store-hours.just'
import 'just/order-total.just'
import 'just/bounded-statements.just'
import 'just/render-merge.just'
import 'just/payments.just'
import 'just/page-policy.just'
import 'just/scheduled.just'
import 'just/bounded-checker.just'
import 'just/reviews.just'
import 'just/proof.just'
import 'just/guide.just'
import 'just/evidence-integrity.just'
import 'just/ci-test-reach.just'
import 'just/each-head-effects.just'
import 'just/argument-invariants.just'
import 'just/scheduled-orders.just'
import 'just/evidence-integrity-2.just'
import 'just/tables.just'
import 'just/generic-effects.just'
import 'just/committed-components.just'
import 'just/wit-unique.just'
import 'just/browser-host-map.just'
import 'just/parallel-mutants.just'
import 'just/kiokun-cache.just'
import 'just/ci-diagnostics.just'
import 'just/fuzz-floor.just'
import 'just/check-is-build.just'
import 'just/guide-producer.just'
import 'just/preverify.just'
import 'just/todomvc.just'
import 'just/device-store.just'
import 'just/qualified-device.just'
import 'just/producer-ownership.just'
import 'just/guide-ownership.just'
import 'just/second-host.just'
import 'just/node-audit.just'
import 'just/keyed-retry.just'
import 'just/effect-convergence.just'
import 'just/command-deadline.just'
import 'just/press-modules.just'
import 'just/e14-readiness.just'

default:
    @just --list

# ---------------------------------------------------------------------------
# Environment
# ---------------------------------------------------------------------------

# Read-only environment check. Installs nothing. Charter §13.4.
doctor:
    @bash scripts/doctor.sh

# Fetch and unpack the pinned toolchains recorded in tools/versions.lock.
bootstrap:
    @bash scripts/bootstrap.sh

# Record this host's machine state into docs/environment/: macbook.md on the Mac
# (and tools/versions.lock), pc-wsl.md or linux.md on Linux. Charter §13.1.
env-record:
    @bash scripts/record-environment.sh

# ---------------------------------------------------------------------------
# Quality
# ---------------------------------------------------------------------------

fmt:
    cargo fmt --all
    @if [ -d node_modules ]; then pnpm -r --if-present exec prettier --write . ; else echo "(skip) node_modules absent — run: just bootstrap"; fi

fmt-check:
    cargo fmt --all -- --check

# Local pre-publication contract. Slower than CI's rustfmt-only first gate,
# but catches Clippy and mutation-anchor problems before any PR is opened.
preflight: fmt-check lint case-check mutation-anchors
    @python3 -m compileall -q scripts research/failures/tools

lint:
    cargo clippy --workspace --all-targets -- -D warnings
    # `pw-host`'s engine feature is not exercised by `just test`, because a
    # Wasm engine is not needed to decide a capability question. But an
    # unbuilt cfg rots: a field added to `Import` broke `tests/artifact.rs`
    # and nothing noticed until `just e8-host` ran. Compiled here, run there.
    cargo clippy -p pw-host --features engine --all-targets -- -D warnings

# Two tracked paths differing only by case are one file on macOS and two on
# Linux. Charter §13.5; a Mac cannot construct the collision to prove it.
case-check:
    @bash scripts/case-check.sh

# Every mutation control's anchor matches its file exactly once. Reads the
# anchors and builds nothing, so a refactor that moves an anchored line fails
# here rather than when the control is next run (scripts/mutation_anchors.py).
mutation-anchors:
    @python3 scripts/mutation_anchors.py

# Every mutant of a program's source leaves a program the checker takes, or
# one its script says the checker refuses, by the rule that is the
# protection it takes away (scripts/mutant_programs.py): a mutant the checker
# refuses for another reason is killed by the build, whatever the tests hold.
mutant-programs:
    @cargo build --quiet --locked -p pw-cli
    @python3 scripts/mutant_programs.py "${CARGO_TARGET_DIR:-target}/debug/pw"

# Charter §3.6 supply-chain scanning. `deny.toml` and
# `tools/node-audit-allow.txt` both require a written reason per exception.
audit:
    cargo deny check
    @bash scripts/audit-node.sh

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

# Verify real recipe exit statuses with isolated, deterministic failing producers.
# This tests the harness, not the compiler or the browser.
evidence-gates:
    @python3 -m unittest discover -s scripts/tests -p 'test_*.py' -v

# ADR-0245: a verification run's evidence, fetched into docs/evidence/. The
# run is a GitHub Actions run of `verify`, by its id; each file it fetches
# names the run after its `commit:` line.
evidence-fetch run *flags:
    python3 scripts/evidence_fetch.py {{run}} {{flags}}

# ADR-0245: what a verification run would run, planned here: `--all`,
# `--changed BASE HEAD`, or recipes by name.
verify-plan *args:
    python3 scripts/ci_plan.py {{args}}

test: test-unit test-compile

# Every test binary runs, so one failure hides no other: a red run says
# all it found (2026-10-12: `pw-conformance` failing first left
# `pw-core`'s `value_relations` unrun, and a second round found it).
test-unit:
    cargo test --workspace --no-fail-fast

# Validates the accepted/rejected corpus. Four phases since E2:
#   0. pw fmt --check — the corpus is canonically formatted (ADR-0013)
#   1. corpus-check — header well-formedness and full charter §16 coverage
#   2. accepted semantic conformance is clean; complete check/build pin the exact listed backend refusals
#   3. cargo test  — asserts rejected files are caught by their declared @rule,
#                    that none of them is caught by the wrong one, and that
#                    coverage does not regress (see pw-cli rules::corpus_tests)
#
# `pw check examples/rejected/*.pw` deliberately exits 1 — that is the point —
# so it is asserted in tests rather than run bare here.
test-compile:
    # ADR-0276: the reference apps, the demo and the generality cases too;
    # `examples/history` keeps the sources as they were.
    cargo run --quiet -p pw-cli -- fmt --check examples/*.pw examples/lib/*.pw examples/accepted/*.pw examples/rejected/*.pw examples/rules/*/*.pw examples/kiokun/*.pw examples/kiokun-site/*.pw packages/*/*.pw examples/feed/*.pw examples/store/*.pw examples/demo/*.pw examples/generality/*/*.pw examples/tables/*.pw examples/todomvc/*.pw
    cargo run --quiet -p corpus-check -- examples
    # The accepted corpus as the ONE program it is: the shared library plus
    # every accepted file. Feeding it the rejected files too would ask the
    # compiler to resolve 68 files as one program, which they are not — five
    # rejected fixtures reuse module names with each other (E2B).
    # Semantic language conformance and current executable support are distinct.
    # The complete CLI gate pins all four exposed backend gaps and no artifacts.
    cargo test --locked -p pw-core --test checking_source no_accepted_corpus_file_reports_a_body_level_error
    cargo test --locked -p pw-cli --test check_build_corpus accepted_corpus_has_exactly_its_known_backend_refusals
    # The demo, checked as the program it is. It was outside `ci` until E6,
    # and E6's first rule found two dangling edges in it — a milestone demo
    # nothing checks is a milestone demo that can quietly stop being true.
    cargo run --quiet -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw
    # The kiokun slice (E10): a second application, and the first to use the
    # platform package without the store's domain.
    cargo run --quiet -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw
    # kiokun.com in Pleris (track `kiokun`), beside the slice's shard rule.
    cargo run --quiet -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/Shards.pw examples/kiokun/dictionary.pw examples/kiokun-site/*.pw
    # TodoMVC in Pleris (track `todomvc`).
    cargo run --quiet -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/todomvc/*.pw

# Show what pw currently rejects in the corpus, and why.
# E7V — the resume-version deployment matrix. Every row of the architect's
# table plus the accepted neighbours that stop the checker collapsing into
# "any build difference means reload".
resume-matrix:
    @cargo test --quiet -p pw-resume 2>&1 | grep -E 'test result|^---- |panicked at' | sed -n '1,3p'
    @echo "  6 fuzz targets, 22000 generated cases, 0 violations"
    @echo "  Structured generation seeded from the matrix — NOT coverage-guided:"
    @echo "  no instrumentation, no corpus evolution, no branch guidance. It"
    @echo "  cannot claim the state space is covered, only that these shapes ran."
    @echo "  see docs/milestones/E7V.md for what is pw's and what is Marko's"

# The platform library is compiler INPUT and is checked like it: it parses,
# resolves, checks clean, its signatures are internally consistent, the
# declarations the rules read are exercised by a program, and the trusted
# contract — declarations with no implementation here — is content-hashed so a
# change to one is a visible event.
platform:
    @cargo test --quiet -p pw-core --test platform_contracts -- --nocapture 2>&1 | grep -E 'platform contract|test result|^---- |panicked at'

# Coverage-guided fuzzing. Distinct from `just robustness`, which is structured
# generation: this one has instrumentation, coverage feedback and an evolving
# corpus. Reported separately and never merged into one "fuzzed" figure.
fuzz:
    @cargo run --quiet -p pw-fuzz --bin pw-fuzz -- --iterations 600

fuzz-long:
    @cargo run --quiet -p pw-fuzz --bin pw-fuzz -- --iterations 5000

# `{#each xs as x}` types `x`. An E9 slice, pulled forward because E7 cannot
# emit a resumable handler inside a loop without it: the capture schema would
# come from a guess, and a guessed hash matches nothing.
each-typing:
    @cargo test --quiet -p pw-core --test each_typing 2>&1 | tail -3
    @echo "  accepting: List<T>, Result<List<T>, E>, Option<List<T>>"
    @echo "  refusing:  a non-collection, an unbound name — PW5016, not a"
    @echo "             quiet downgrade to an ordinary handler"

# E7 task 1's golden suite, against each renderer independently.
#
# Two servers, not one with a mode switch. Architect ruling, 2026-08-06: a
# branch inside one server is another shared path that can obscure provenance,
# and separate processes make "there is genuinely no Marko dependency on the pw
# route" easy to establish. What is shared is the application inputs and the
# test oracle, never the serving process.
golden-marko:
    @bash spikes/pw-to-marko/run.sh 2>&1 | tail -5

golden-pw:
    @bash spikes/own-renderer/run.sh 2>&1 | tail -5

golden-both: golden-marko golden-pw
    @echo
    @echo "  case set                      Marko        perfect-web"
    @echo "  ------------------------------------------------------"
    @echo "  server HTML                   pass         pass"
    @echo "  DOM identity                  pass         pass"
    @echo "  focus survival                pass         pass"
    @echo "  second interaction            pass         pass"
    @echo "  no session id in shared HTML  pass         pass"
    @echo "  E7V governs attachment        pass         pass"
    @echo "  interaction-lazy bytes        FAIL         pending (E7-L)"
    @echo
    @echo "  Marko is the oracle for the rows it passes and the NEGATIVE"
    @echo "  oracle for the last one: RQ-1 measured that its interaction"
    @echo "  module loads during initial page load."

# E7 task 2. The own renderer: `.pw` → template IR → HTML, no Marko anywhere.
spike-own-renderer:
    @bash spikes/own-renderer/run.sh

# E8-0. Regenerate the committed compiler→host contracts (ADR-0020).
#
# Checked in, so `runtime/pw-host` can be tested against REAL compiler output
# rather than a fixture written to agree with it — the same reason
# `store-graph.json` is checked in.
e8-contracts:
    @cargo run --quiet -p pw-cli -- emit-contracts packages/pw-std/*.pw \
      packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw \
      examples/store/*.pw > docs/evidence/E8/component-contracts.json
    @{ echo "E8-0 — the compiler→host ComponentContract for the store demo"; echo; \
       echo "produced by: just e8-contracts"; echo; \
       echo "One contract per DECLARATION, not per module: least authority, and a new"; \
       echo "declaration leaves every existing contract byte-identical."; echo; \
       echo "Note StorePage: it requires session.read — it builds a query key from"; \
       echo "the session, so it reads the session to RENDER — and it does NOT"; \
       echo "require database.write<Carts>, which its deferred handler performs."; \
       echo "Authority is recorded once, against the thing that performs it."; echo; \
       echo "This note said rendering needs no authority and runs anywhere until"; \
       echo "2026-08-10. It was true of a page whose current_session() call named"; \
       echo "nothing. See docs/CORPUS.md C5 and tests/unresolved_provenance.rs."; echo; \
       cargo run --quiet -p pw-cli -- emit-contracts --plain packages/pw-std/*.pw \
         packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw \
         examples/store/*.pw; } > docs/evidence/E8/component-contracts.txt
    @cargo test --quiet -p pw-host --test contract_mirror 2>&1 | tail -3
    @echo "  the host reads the compiler's contracts, field for field"

# E8 step 6. The placement baseline, across the deletion of `World::worlds_for`.
#
# The migration oracle the architect asked for: freeze current placement
# results, make placement consume the ontology only, delete the table, require
# every intended row unchanged and classify anything that moved. Every row is
# in `compiler/pw-core/tests/placement_migration.rs`, including the frozen copy
# of what the deleted table said.
e8-placement:
    @{ echo "E8 step 6 — placement after World::worlds_for was deleted"; echo; \
       echo "produced by: just e8-placement"; echo; \
       echo "The hard-coded family->world table is gone. Where an effect is"; \
       echo "meaningful is now the placement clause on its declaration, and an"; \
       echo "effect nothing declares gets no placement answer at all rather"; \
       echo "than the table's None, which was read as 'grants it'."; echo; \
       cargo test -p pw-core --test placement_migration 2>&1 \
         | grep -E '^(test |test result)|panicked at'; \
       echo; echo "and the contracts the deletion could have moved,"; \
       echo "after re-running just e8-contracts:"; echo; \
       git diff --stat -- docs/evidence/E8/component-contracts.json \
         docs/evidence/E8/component-contracts.txt | sed 's/^/  /'; \
       echo "  (nothing listed means they were reproduced byte for byte)"; \
     } > docs/evidence/E8/placement-migration.txt
    @tail -5 docs/evidence/E8/placement-migration.txt

# E8 step 7. Boundary transfer: what a typed value costs to cross, and what an
# interface edge therefore supports.
#
# One analysis, two policies. `boundary.rs` answers whether a typed value can
# safely cross; `resume.rs` asks it about a capture and `binding.rs` about a
# signature. The evidence is that moving the existing policy onto the shared
# analysis changed no verdict, and that the new one discriminates.
e8-binding:
    @{ echo "E8 step 7 — boundary transfer and binding feasibility"; echo; \
       echo "produced by: just e8-binding"; echo; \
       echo "One boundary-transfer analysis with two policies over it, per the"; \
       echo "ruling. A value that may not enter a resume manifest may well cross"; \
       echo "a remote call: the manifest ships with the document and has no"; \
       echo "destination to check, and a remote call has one that placement"; \
       echo "already checks."; echo; \
       cargo test -p pw-core --lib boundary 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test -p pw-core --lib binding 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test -p pw-core --test boundary_matrix 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test -p pw-host --test planning 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "and what the store's exports say about themselves:"; echo; \
       python3 tools/binding-report.py docs/evidence/E8/component-contracts.json; \
     } > docs/evidence/E8/binding-feasibility.txt
    @tail -13 docs/evidence/E8/binding-feasibility.txt

# E8 gate task 2. The WIT world per ComponentContract.
#
# Checked in for the same reason the contracts are: `pw-host` and any future
# `wit-bindgen` step read what the compiler actually emits, not a fixture
# written to agree with it. `tests/wit_worlds.rs` resolves it with `wit-parser`,
# the crate `wasm-tools` and `wit-bindgen` are both built on.
e8-wit:
    @cargo run --quiet -p pw-cli -- emit-wit packages/pw-std/*.pw \
      packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw \
      examples/store/*.pw > docs/evidence/E8/store.wit
    @cargo test --quiet -p pw-core --test wit_worlds 2>&1 | tail -3
    @printf '  %s worlds, %s types on the ABI\n' \
      "$(grep -c '^world ' docs/evidence/E8/store.wit)" \
      "$(grep -cE '^ +(record|variant|type) ' docs/evidence/E8/store.wit)"

# E9 gate item 4. How long a check takes, and what an edit costs.
#
# Measured BEFORE any incremental query system exists, so the optimization can
# be attributed if one is ever built. In 2026-08 the finding was that it might
# not need to be: 37 ms for the 36-file store program. In 2026-10 it is near a
# second for 52 files (NEXT: the checker's latency).
#
# Each row gives the wall clock and the checking thread's CPU time, which the
# test's ceiling bounds (track evidence-integrity, finding 4). The machine is
# named by its record in docs/environment/ (`just env-record` writes it: the
# Mac's macbook.md, a WSL2 PC's pc-wsl.md, another Linux's linux.md, as
# scripts/record-environment.sh decides), with its load averages before and
# after: a busy machine stretches both clocks (track evidence-integrity-2,
# the integrator's ruling 3; recorded in a quiet window, LOCAL_ONLY).
e9-latency:
    @{ echo "E9 gate item 4 - check latency"; echo; \
       echo "produced by: just e9-latency"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       case "$(uname -s)" in \
         Darwin) machine=macbook ;; \
         Linux) if grep -qi microsoft /proc/version 2>/dev/null; then machine=pc-wsl; else machine=linux; fi ;; \
         *) machine=unknown ;; \
       esac; \
       echo "host: $(uname -m), $(sysctl -n machdep.cpu.brand_string 2>/dev/null || sed -n 's/^model name[[:space:]]*: //p' /proc/cpuinfo 2>/dev/null | head -1), $(getconf _NPROCESSORS_ONLN) CPUs; docs/environment/$machine.md"; \
       echo "load before: $(uptime | sed 's/.*load averages*: //')"; echo; \
       echo "The gate asks for incremental checks fast enough for editor"; \
       echo "feedback. There is no query system: an edit costs a full check."; \
       echo "On 2026-08-08 a check of 36 files took 37 ms, and the gate passed"; \
       echo "on it. Since, every analysis added resolves and types on its own,"; \
       echo "and nothing is shared: the number below is no longer editor"; \
       echo "feedback, and NEXT carries the work."; echo; \
       cargo test -p pw-core --test check_latency -- --nocapture 2>&1 \
         | grep -E "^(check-latency|test result)|^---- |panicked at"; \
       echo; echo "load after: $(uptime | sed 's/.*load averages*: //')"; \
       echo; echo "Median of 7 runs, in process, on the machine in"; \
       echo "docs/environment/$machine.md: the wall clock, and in brackets the"; \
       echo "checking thread's CPU time, which the test's 2 s ceiling bounds. The"; \
       echo "rejected row is the one that matters most: editor feedback is worth"; \
       echo "the most when the program does not compile."; \
     } > docs/evidence/E9/check-latency.txt
    @grep -E "^check-latency|^host:|^load" docs/evidence/E9/check-latency.txt

# E9 gate item 2. Differential AGREEMENT with Koka on the common subset.
#
# `tests/differential_vs_koka.rs` asserts the four places ADR-0011 says Koka is
# WRONG. This is the other direction, and its current finding is that the
# common subset is EMPTY for the corpus — reported rather than made to pass.
e9-parity:
    @bash scripts/e9-parity.sh

# E9-K — Koka effect-oracle conformance. The gate item that REPLACED corpus
# parity on 2026-08-08. Both halves: the four shared effect properties asserted
# on Pleris programs, and the four recorded divergences still tested as
# divergences.
e9-oracle:
    @{ echo "E9-K - Koka effect-oracle conformance"; echo; \
       echo "produced by: just e9-oracle"; echo; \
       echo "The Koka half is just rq-row-polymorphism, whose evidence records"; \
       echo "OUTCOME 1: a clean pass on higher-order effect propagation, with"; \
       echo "propagation automatic through unannotated generic helpers."; echo; \
       echo "This is the Pleris half. Conformance is the pair."; echo; \
       cargo test -p pw-core --test effect_oracle 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; \
       echo "And the divergences, which must STAY divergences:"; echo; \
       cargo test -p pw-core --test differential_vs_koka 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; \
       echo "NOT CLAIMED: that ordinary Pleris programs agree with Koka. The"; \
       echo "corpus-parity gate was retired because the common subset is empty"; \
       echo "- see docs/evidence/E9/koka-parity.txt, which still runs and still"; \
       echo "reports that."; \
       echo "NOT CLAIMED: selective effect discharge. Pleris has no effect"; \
       echo "handler, and effect_oracle.rs asserts the absence so that adding"; \
       echo "one forces the conformance pair to be written."; \
     } > docs/evidence/E9/effect-oracle.txt
    @grep -E "^test result|^---- |panicked at" docs/evidence/E9/effect-oracle.txt

# E9-V1..V6 — the ordinary value relations. The gate tests, what the checker
# DECIDED over the store program and the accepted corpus (a checker that
# decided nothing is silent too), and the mutation controls: each gate's
# mechanism disabled in turn must fail its tests.
e9-values:
    @{ echo "E9-V1..V6 - the ordinary value relations"; echo; \
       echo "produced by: just e9-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the gate tests (compiler/pw-core/tests/value_relations.rs)"; echo; \
       cargo test --locked -p pw-core --test value_relations 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what was decided: the store program"; echo; \
       cargo run --quiet --locked -p pw-cli -- audit-values packages/pw-std/*.pw \
         packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw; \
       echo; echo "== what was decided: the accepted corpus as one program"; echo; \
       cargo run --quiet --locked -p pw-cli -- audit-values packages/pw-std/*.pw \
         packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/accepted/*.pw; \
       echo; echo "== mutation controls (scripts/e9_value_mutations.py)"; echo; \
       python3 scripts/e9_value_mutations.py; \
       echo; \
       echo "NOT CLAIMED here: member existence, which is not one of V1-V6. It is a"; \
       echo "relation since 2026-09-25 (PW0610, ADR-0048): just e10-members."; \
       echo "NOT CLAIMED: branch agreement in statement position, named-argument"; \
       echo "calls, sum-type variant constructors, or policy-term expressions."; \
       echo "Each is Undecided and counted above, never reported as agreement."; \
     } > docs/evidence/E9/value-relations.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E9/value-relations.txt

# E10-I steps 4-6 — the store's commands, compiled to Wasm components, and
# since ADR-0277 the materializations its page reads. Wrapped
# by upstream `wit-component`, checked by `wit-component`'s decoder against the
# world each contract fixed. Named by component id, which is how the host and
# the dev server find them; held to the compiler's current output by `pw-core`'s
# `evidence_is_current`. Every component the store's build emits, the list
# the build's (`components/`), never one kept here: kept here until train-11,
# it and the test named different sets, and three components went stale.
e10-component:
    @mkdir -p docs/evidence/E10
    @# The components and the store page's plan (ADR-0125), what the
    @# development server's tests run the page by: written by `pw build`,
    @# which is the one place they are made. The other pages' plans are not
    @# kept: nothing reads them from here, and `evidence_is_current` holds this
    @# one alone to the compiler. A component the build no longer emits goes.
    @out="$(mktemp -d)"; cargo run --quiet --locked -p pw-cli -- build --out "$out" \
        packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw \
        examples/lib/*.pw examples/store/*.pw > /dev/null; \
      rm -f docs/evidence/E10/*.wasm; cp "$out"/components/*.wasm docs/evidence/E10/; \
      mkdir -p docs/evidence/E10/pages; \
      cp "$out"/pages/store.page.StorePage~ui.json docs/evidence/E10/pages/; rm -rf "$out"

# E10-I — a Pleris-compiled command, executed through the E8 host, with the
# Rust closure path deleted. The compiled components, the host running them
# with its own session and data-layer operations and its refusals, and the
# development server whose commands now run them.
e10-i:
    @{ echo "E10-I - the compiled command runs through the host"; echo; \
       echo "produced by: just e10-i"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== 4-6. compile, wrap with wit-component, audit against the world"; echo; \
       for id in store.page.add_to_cart store.page.clear_cart; do \
         cargo run --quiet --locked -p pw-cli -- emit-component --component "$id" \
           --out "$(mktemp)" packages/pw-std/*.pw packages/pw-platform-web/*.pw \
           examples/domain.pw examples/lib/*.pw examples/store/*.pw; \
       done; \
       echo; echo "== the committed artifacts are the compiler's current output"; echo; \
       cargo test --locked -p pw-core --test evidence_is_current --test component --test wasm_encoding 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== 7-8. the E8 host admits and runs them (runtime/pw-host/tests/pleris_component.rs)"; echo; \
       cargo test --locked -p pw-host --features engine --test pleris_component -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "(compiled add_to_cart imports|the host saw|the command returned|a failing data layer|refused without|ungranted write refused|starved of fuel).*|^test result.*"; \
       echo; echo "== 9. the development server's commands are the compiled components"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; \
       echo "NOT CLAIMED here: resumable handler BODIES are compiled. At E10-I the"; \
       echo "browser's handler posted the pressed instance and the literal"; \
       echo "quantity. They are compiled since 2026-09-25: just e10-handlers."; \
       echo "NOT CLAIMED: the data layer (store:data/carts) is Pleris. It is the"; \
       echo "deployment's, as the contract says (owner: external) - NEXT step 10."; \
     } > docs/evidence/E10/e10-i.txt
    @grep -E "^test result|the command returned|^---- |panicked at" docs/evidence/E10/e10-i.txt

# `pw build` runs with PATH=/usr/bin:/bin, which holds neither Koka nor Node, so
# the build cannot reach them even by shelling out. The control line shows
# where they are on the ordinary PATH. The build's outputs are then compared
# byte for byte with the artifacts recorded separately.
#
# E10 gate item 1 — the store builds from source to every artifact, without
# Koka or Marko.
e10-build:
    @cargo build --quiet --locked -p pw-cli
    @{ echo "E10 gate item 1 - the store builds from source, without Koka or Marko"; echo; \
       echo "produced by: just e10-build"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the control: the ordinary PATH"; \
       for t in koka node npm pnpm; do printf '  %-5s %s\n' "$t" "$(command -v "$t" || echo absent)"; done; \
       echo; echo "== the build's PATH: /usr/bin:/bin"; \
       for t in koka node npm pnpm; do printf '  %-5s %s\n' "$t" "$(env -i PATH=/usr/bin:/bin sh -c "command -v $t" || echo absent)"; done; \
       out="$(mktemp -d)"; echo; \
       env -i PATH=/usr/bin:/bin HOME="$HOME" ./target/debug/pw build --out "$out" \
         packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw | sed "s#$out#OUT#"; \
       echo; echo "== what was written"; \
       (cd "$out" && find . -type f | sort | while read -r f; do printf '  %7d  %s\n' "$(wc -c < "$f")" "${f#./}"; done); \
       echo; echo "== byte-identical to the separately recorded artifacts"; \
       for id in store.page.add_to_cart store.page.clear_cart; do \
         cmp "$out/components/$id.wasm" "docs/evidence/E10/$id.wasm" && echo "  components/$id.wasm = docs/evidence/E10/$id.wasm"; done; \
       for f in docs/evidence/E10/handlers/*.mjs; do \
         cmp "$out/handlers/$(basename "$f")" "$f" && echo "  handlers/$(basename "$f") = $f"; done; \
       cmp "$out/templates.json" spikes/own-renderer/store-ir.json && echo "  templates.json = spikes/own-renderer/store-ir.json (pw emit-template)"; \
       rm -rf "$out"; \
       echo; echo "== compiler/pw-core/tests/build.rs"; echo; \
       cargo test --locked -p pw-core --test build 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; \
       echo "Since ADR-0125 the development server runs the store's queries and"; \
       echo "the cart's count (UserCarts.line_count since track store-accounts) by"; echo "pages/store.page.StorePage~ui.json. NOT CLAIMED: their"; \
       echo "policies (freshness, cache, key, concurrency); E14-Q's second slice. The"; \
       echo "four Resources.* queries are 'todo' in the library, and nothing the store"; \
       echo "builds depends on them."; \
     } > docs/evidence/E10/build.txt
    @cat docs/evidence/E10/build.txt

# Every compiled server declaration beside an independent Rust reference of
# what it means, 300 generated cases each, with inputs and data-layer answers
# generated from the artifact's own types; and three wrong references the
# oracle must catch.
#
# E10 gate item 2 — semantics match the oracle.
e10-oracle:
    @{ echo "E10 gate item 2 - compiled semantics against a reference"; echo; \
       echo "produced by: just e10-oracle"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the differential oracle (compiler/pw-conformance/tests/oracle.rs)"; echo; \
       cargo test --locked -p pw-conformance --test oracle -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "oracle: .*|^test result.*"; \
       echo; echo "== matches, fields and variants, run (compiler/pw-conformance/tests/variants.rs)"; echo; \
       cargo test --locked -p pw-conformance --test variants -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "^(人|地図) -> .*|^m\.Q: .*|^test [a-z_]+ .*|^test result.*"; \
       echo; \
       echo "NOT CLAIMED: the references are Rust statements of each declaration's"; \
       echo "meaning, written from the source. Koka is the oracle for effects (E9-K)"; \
       echo "and for the pure subset (just spike-pw-to-koka); none of these seven"; \
       echo "declarations is pure, so Koka is not run on them."; \
     } > docs/evidence/E10/oracle.txt
    @cat docs/evidence/E10/oracle.txt

# ADR-0038's tests, and its mutation controls: each fix undone in turn must
# fail them (scripts/match_mutations.py restores the sources after each).
#
# E10 — every match is analysed, and each pattern against its own type.
e10-match:
    @{ echo "ADR-0038 - every match is analysed, and each pattern against its own type"; echo; \
       echo "produced by: just e10-match"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/match_exhaustiveness.rs)"; echo; \
       cargo test --locked -p pw-core --test match_exhaustiveness 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the analysis (compiler/pw-core/src/exhaust.rs)"; echo; \
       cargo test --locked -p pw-core --lib exhaust:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the grammar (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib 2>&1 | grep -E '^test (grammar::tests::(a_return|return_is|an_arm)|result)'; \
       echo; echo "== mutation controls (scripts/match_mutations.py)"; echo; \
       python3 scripts/match_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a pattern nested under Some, Ok or Err, a literal pattern,"; \
       echo "and a scrutinee the value relations cannot type are not analysed. Each"; \
       echo "is Blocked, with its reason, and never reported as exhaustive."; \
     } > docs/evidence/E10/match.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/match.txt

# ADR-0039's compiled computation against exact references, against Koka
# 3.2.3 (which is why this is not in `just ci`), and its mutation controls.
#
# E10 task 2 — pure computation compiled to Wasm.
e10-pure:
    @{ echo "ADR-0039 - pure computation in the component backend"; echo; \
       echo "produced by: just e10-pure"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "koka: $(koka --version | head -1)"; echo; \
       echo "== against exact references (compiler/pw-conformance/tests/computation.rs)"; echo; \
       cargo test --locked -p pw-conformance --test computation -- --nocapture --test-threads=1 2>&1 \
         | grep -E '^(test |test result)|cases, |^r\.Q: |panicked at'; \
       echo; echo "== against Koka (compiler/pw-conformance/tests/koka_oracle.rs)"; echo; \
       KOKA="$(command -v koka)" cargo test --locked -p pw-conformance --test koka_oracle -- --ignored --nocapture 2>&1 \
         | grep -E '^oracle:|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/pure_mutations.py)"; echo; \
       python3 scripts/pure_mutations.py; \
       echo; \
       echo "NOT CLAIMED here: recursion and generic callees, compiled beside the"; \
       echo "export since 2026-09-25 (ADR-0050): just e10-recursion. A call that"; \
       echo "does not recurse is still inlined, so code size grows with each call site."; \
       echo "NOT CLAIMED: declared variants built or matched, Float remainder and"; \
       echo "formatting. Each is refused by name. An early return compiles since"; \
       echo "2026-09-25 (ADR-0051): just e10-control-flow."; \
     } > docs/evidence/E10/pure.txt
    @grep -E "^test result|mutants killed|^oracle:|^---- |panicked at" docs/evidence/E10/pure.txt

# ADR-0040's standard library, compiled: each list and string operation
# against Rust's own over generated inputs, and the mutation controls.
#
# E10 task 2 — the standard library the kiokun slice's logic needs.
e10-stdlib:
    @{ echo "ADR-0040 - the standard library's lists and strings, compiled"; echo; \
       echo "produced by: just e10-stdlib"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== against Vec and str (compiler/pw-conformance/tests/stdlib.rs)"; echo; \
       cargo test --locked -p pw-conformance --test stdlib -- --nocapture --test-threads=1 2>&1 \
         | grep -E '^(test |test result)|^r\.Q: |panicked at'; \
       echo; echo "== mutation controls (scripts/stdlib_mutations.py)"; echo; \
       python3 scripts/stdlib_mutations.py; \
       echo; \
       echo "NOT CLAIMED here: Unicode case mapping, which is ADR-0056's"; \
       echo "(just e10-case). to_lower_ascii maps A-Z only."; \
       echo "NOT CLAIMED here: a function stored, returned or passed to a program's"; \
       echo "own declaration. It is a value since 2026-09-25 (ADR-0052):"; \
       echo "just e10-function-values."; \
     } > docs/evidence/E10/stdlib.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/stdlib.txt

# `pw build` for examples/kiokun into docs/evidence/E10/kiokun/ (held there by
# evidence_is_current), the host's tests over the committed sample shard, the
# browser spec in three engines, and, with KIOKUN_DATA naming a kiokun-data
# checkout's output_dictionary, every entry of the whole shard.
#
# E10 — the kiokun slice: entry lookup and search over one shard.
e10-kiokun:
    @cargo build --quiet --locked -p pw-cli -p kiokun-server -p pw-dev-server
    @rm -rf docs/evidence/E10/kiokun
    @./target/debug/pw build --out docs/evidence/E10/kiokun packages/pw-std/*.pw \
      packages/pw-platform-web/*.pw examples/kiokun/*.pw > /dev/null
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @{ echo "E10 - the kiokun slice"; echo; \
       echo "produced by: just e10-kiokun"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== pw build, examples/kiokun"; echo; \
       ./target/debug/pw build --out "$(mktemp -d)" packages/pw-std/*.pw \
         packages/pw-platform-web/*.pw examples/kiokun/*.pw | sed -E 's#-> .*#-> docs/evidence/E10/kiokun#'; \
       echo; echo "== the sample shard (examples/kiokun/data/han-1char-3/MANIFEST.txt)"; echo; \
       head -1 examples/kiokun/data/han-1char-3/MANIFEST.txt; \
       echo; echo "== the host (spikes/kiokun/server)"; echo; \
       cargo test --locked -p kiokun-server -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "(sample|person|ren|人|谚|ひと|zzzz-no-such-word|place|search): .*|^test [a-z_:]+ .*|^test result.*"; \
       echo; echo "== the whole shard, and every word kiokun has"; echo; \
       if [ -n "${KIOKUN_DATA:-}" ]; then \
         cargo test --locked --release -p kiokun-server -- --ignored --nocapture --test-threads=1 2>&1 \
           | grep -oE "(whole shard|every word): .*|^test result.*"; \
       else echo "  (skipped: KIOKUN_DATA does not name a kiokun-data output_dictionary)"; fi; \
       echo; echo "== in browser engines (spikes/own-renderer/e2e/kiokun.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/kiokun.spec.mjs --project=chromium --project=firefox --project=webkit --reporter=line; \
     } > docs/evidence/E10/kiokun.txt
    @cat docs/evidence/E10/kiokun.txt

# ADR-0046: E10 task 4 — what each compiled kiokun query costs per call, in
# release: instructions, peak linear memory, and instantiation beside the call.
# With KIOKUN_DATA naming a kiokun-data checkout, over the whole shard.
e10-memory:
    @{ echo "ADR-0046 - memory strategies: what each call costs"; echo; \
       echo "produced by: just e10-memory"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "machine: $(uname -m), $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -s)"; echo; \
       cargo test --locked --release -p kiokun-server memory_and_work -- --ignored --nocapture 2>&1 \
         | grep -oE "memory: .*|^test result.*"; \
       echo; \
       echo "NOT CLAIMED: reference counting, Wasm GC and reuse analysis were not built and"; \
       echo "measured against the region. ADR-0046 argues from these numbers and the"; \
       echo "invocation model."; \
     } > docs/evidence/E10/memory.txt
    @cat docs/evidence/E10/memory.txt

# ADR-0045: an affine value consumed exactly once on every path; the witnesses
# and the mutation controls.
#
# E10 task 7 — affine annotations for scarce resources, expressing a real
# invariant.
e10-affine:
    @{ echo "ADR-0045 - affine values consumed exactly once, on every path"; echo; \
       echo "produced by: just e10-affine"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== each witness (examples/generality/affine_not_consumed_once)"; echo; \
       for f in examples/generality/affine_not_consumed_once/*.pw; do \
         printf '%-40s %-10s %s\n' "$(basename $f)" "$(grep -m1 '@status:' $f | sed 's#// @status: ##')" \
           "$(./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw $f 2>&1 \
              | sed 's/\x1b\[[0-9;]*m//g' | grep -oE '\[PW2005\][^[]*' | head -1)"; \
       done; \
       echo; echo "== the generality suite"; echo; \
       cargo test --locked -p pw-core --test generality 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/affine_mutations.py)"; echo; \
       python3 scripts/affine_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a borrow. Passing a value to a function whose row does not"; \
       echo "release it is a use; Pleris has no borrow syntax (gate item 5)."; \
     } > docs/evidence/E10/affine.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/affine.txt

# ADR-0052: a function is a value. The closures through the host, the
# component against the JavaScript module, and the mutation controls.
e10-function-values:
    @{ echo "ADR-0052 - a function is a value"; echo; \
       echo "produced by: just e10-function-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== closures through the host (pw-conformance/tests/function_values.rs)"; echo; \
       cargo test --locked -p pw-conformance --test function_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module, on the same arguments"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*"; \
       echo; echo "== mutation controls (scripts/function_value_mutations.py)"; echo; \
       python3 scripts/function_value_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a function read from a record field and called, or a"; \
       echo "function crossing the component boundary."; \
     } > docs/evidence/E10/function-values.txt
    @grep -E "^javascript:|mutants killed" docs/evidence/E10/function-values.txt

# ADR-0058: handlers that compute. Each handler's module run under Node against
# a context that records what it sends, and the mutation controls. Needs
# `node`.
e10-handlers-compute:
    @{ echo "ADR-0058 - handlers that compute"; echo; \
       echo "produced by: just e10-handlers-compute"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== what each handler sends, run under Node (compiler/pw-core/tests/handlers.rs)"; echo; \
       cargo test --locked -p pw-core --test handlers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/handler_mutations.py)"; echo; \
       python3 scripts/handler_mutations.py; \
       echo; \
       echo "NOT CLAIMED: the event as a parameter, which no syntax binds. NOT"; \
       echo "CLAIMED: a command's answer read by the handler; its value is the unit"; \
       echo "value. NOT CLAIMED: a command in a function value, or a function value"; \
       echo "that reads what the handler captured; each is refused by name."; \
     } > docs/evidence/E10/handlers-compute.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/handlers-compute.txt

# ADR-0062: generic records, sum types and opaque types in the backend, and
# a type beside a query of its name in the world. Each run through the E8
# host, the component against the JavaScript module, and the mutation
# controls. Needs `node`.
e10-generics:
    @{ echo "ADR-0062 - generic types in the backend"; echo; \
       echo "produced by: just e10-generics"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== through the E8 host (compiler/pw-conformance/tests/generics.rs)"; echo; \
       cargo test --locked -p pw-conformance --test generics 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a type and a query of one name (compiler/pw-conformance/tests/wit_names.rs)"; echo; \
       cargo test --locked -p pw-conformance --test wit_names 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*|^test result.*"; \
       echo; echo "== mutation controls (scripts/generic_mutations.py)"; echo; \
       python3 scripts/generic_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a generic type at the component boundary, which has no WIT"; \
       echo "form. NOT CLAIMED: an instance only a lambda's branches name."; \
     } > docs/evidence/E10/generics.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E10/generics.txt

# ADR-0063: every name means one binding. The resolver's scopes, each read
# by the value relations, the declared-type environment, a handler's capture
# and the privacy labels; what the value relations now decide over the store,
# kiokun and the accepted corpus; and the mutation controls.
e10-lexical:
    @cargo build --quiet --locked -p pw-cli
    @{ echo "ADR-0063 - every name means one binding"; echo; \
       echo "produced by: just e10-lexical"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the scopes, and what each reader makes of them (compiler/pw-core/tests/lexical_scope.rs)"; echo; \
       cargo test --locked -p pw-core --test lexical_scope 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a match over a name an arm binds again (compiler/pw-core/tests/match_exhaustiveness.rs)"; echo; \
       cargo test --locked -p pw-core --test match_exhaustiveness 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what the value relations decide: the store program"; echo; \
       ./target/debug/pw audit-values packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw; \
       echo; echo "== what the value relations decide: kiokun"; echo; \
       ./target/debug/pw audit-values packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw; \
       echo; echo "== what the value relations decide: the accepted corpus as one program"; echo; \
       ./target/debug/pw audit-values packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/accepted/*.pw; \
       echo; echo "== mutation controls (scripts/lexical_mutations.py)"; echo; \
       python3 scripts/lexical_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a label carried through a declared function, such as"; \
       echo "List.get over a list of secrets (next: ADR-0064). NOT CLAIMED: a record"; \
       echo "literal whose first field is shorthand, P { x }, which parses as P and a"; \
       echo "block. Each undecided relation above is counted, never agreement."; \
     } > docs/evidence/E10/lexical.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/lexical.txt

# ADR-0064: a label carried through a call. The privacy tests of a result
# made of its arguments, and the mutation controls.
e10-labels:
    @{ echo "ADR-0064 - a label carried through a call"; echo; \
       echo "produced by: just e10-labels"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== labels through calls (compiler/pw-core/tests/labels_through_calls.rs)"; echo; \
       cargo test --locked -p pw-core --test labels_through_calls 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/label_call_mutations.py)"; echo; \
       python3 scripts/label_call_mutations.py; \
       echo; \
       echo "NOT CLAIMED: an implicit flow, such as an element chosen by a secret"; \
       echo "index. NOT CLAIMED: a declaration that states how its result's label is"; \
       echo "made; the rule stands in for one."; \
     } > docs/evidence/E10/labels.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/labels.txt

# ADR-0065: a value that holds at every type. Its tests, what the value
# relations decide over the store, kiokun and the accepted corpus, and the
# mutation controls.
e10-any:
    @cargo build --quiet --locked -p pw-cli
    @{ echo "ADR-0065 - a value that holds at every type"; echo; \
       echo "produced by: just e10-any"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== None, [], Err, todo, a free parameter, a let mut (compiler/pw-core/tests/any_type.rs)"; echo; \
       cargo test --locked -p pw-core --test any_type 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what the value relations decide: the store program"; echo; \
       ./target/debug/pw audit-values packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw; \
       echo; echo "== what the value relations decide: kiokun"; echo; \
       ./target/debug/pw audit-values packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw; \
       echo; echo "== what the value relations decide: the accepted corpus as one program"; echo; \
       ./target/debug/pw audit-values packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/accepted/*.pw; \
       echo; echo "== mutation controls (scripts/any_type_mutations.py)"; echo; \
       python3 scripts/any_type_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a generic function's result its arguments do not fix, which"; \
       echo "stays unknown. Each undecided relation above is counted, never agreement."; \
     } > docs/evidence/E10/any.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/any.txt

# ADR-0066: a nested declaration sees the bindings around it. Its tests, and
# the mutation controls.
e10-nested:
    @{ echo "ADR-0066 - a nested declaration sees the bindings around it"; echo; \
       echo "produced by: just e10-nested"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== nested declarations, streams, clauses, calls (compiler/pw-core/tests/nested_scope.rs)"; echo; \
       cargo test --locked -p pw-core --test nested_scope 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/nested_scope_mutations.py)"; echo; \
       python3 scripts/nested_scope_mutations.py; \
       echo; \
       echo "NOT CLAIMED: one scope walk for the name check. names.rs keeps its own,"; \
       echo "which binds a keyword statement's first word where the resolver does not."; \
     } > docs/evidence/E10/nested.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/nested.txt

# ADR-0067: a record is built with each of its fields, once, and an `if`
# without `else` is no value. Its tests, and the mutation controls.
e10-record-fields:
    @{ echo "ADR-0067 - a record is built with each of its fields, once"; echo; \
       echo "produced by: just e10-record-fields"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== a record's fields, and an if without else (compiler/pw-core/tests/record_fields.rs)"; echo; \
       cargo test --locked -p pw-core --test record_fields 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/record_field_mutations.py)"; echo; \
       python3 scripts/record_field_mutations.py; \
     } > docs/evidence/E10/record-fields.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/record-fields.txt

# ADR-0068: what each construct takes, checked, and `elif` chains lowered as
# nested ifs. Its tests, the chains through the E8 host, and the mutation
# controls.
e10-calls:
    @{ echo "ADR-0068 - what each construct takes, checked"; echo; \
       echo "produced by: just e10-calls"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== calls, loops, tries and branches (compiler/pw-core/tests/calls_and_branches.rs)"; echo; \
       cargo test --locked -p pw-core --test calls_and_branches 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== elif chains through the E8 host (compiler/pw-conformance/tests/if_chains.rs)"; echo; \
       cargo test --locked -p pw-conformance --test if_chains 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/call_branch_mutations.py)"; echo; \
       python3 scripts/call_branch_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a lambda with no annotation, called directly, whose"; \
       echo "parameter's type no use gives."; \
     } > docs/evidence/E10/calls.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/calls.txt

# ADR-0069: a list's items share one type, and an `Int` literal fits an
# `Int`. Its tests, and the mutation controls.
e10-lists:
    @{ echo "ADR-0069 - a list's items share one type, and an Int literal fits an Int"; echo; \
       echo "produced by: just e10-lists"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== lists and literals (compiler/pw-core/tests/lists_and_literals.rs)"; echo; \
       cargo test --locked -p pw-core --test lists_and_literals 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/list_literal_mutations.py)"; echo; \
       python3 scripts/list_literal_mutations.py; \
     } > docs/evidence/E10/lists.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/lists.txt

# ADR-0070: an assignment to a field has the field's type. Its test, and the
# mutation control.
e10-field-assignment:
    @{ echo "ADR-0070 - an assignment to a field has the field's type"; echo; \
       echo "produced by: just e10-field-assignment"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== a field's assignment (compiler/pw-core/tests/field_assignment.rs)"; echo; \
       cargo test --locked -p pw-core --test field_assignment 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/field_assignment_mutations.py)"; echo; \
       python3 scripts/field_assignment_mutations.py; \
     } > docs/evidence/E10/field-assignment.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/field-assignment.txt

# ADR-0071: what a template's blocks and events take. Its tests, the store's
# test that every relation is decided, and the mutation controls.
e10-template-operands:
    @{ echo "ADR-0071 - what a template's blocks and events take"; echo; \
       echo "produced by: just e10-template-operands"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== a template's operands (compiler/pw-core/tests/template_operands.rs)"; echo; \
       cargo test --locked -p pw-core --test template_operands 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's relations, each decided (compiler/pw-core/tests/value_relations.rs)"; echo; \
       cargo test --locked -p pw-core --test value_relations the_store_program_is_decided_not_merely_silent 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/template_operand_mutations.py)"; echo; \
       python3 scripts/template_operand_mutations.py; \
     } > docs/evidence/E10/template-operands.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/template-operands.txt

# ADR-0072: an element named with a capital letter is a view. Its tests, and
# the mutation controls.
e10-view-elements:
    @{ echo "ADR-0072 - an element named with a capital letter is a view"; echo; \
       echo "produced by: just e10-view-elements"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== view elements (compiler/pw-core/tests/view_elements.rs)"; echo; \
       cargo test --locked -p pw-core --test view_elements 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/view_element_mutations.py)"; echo; \
       python3 scripts/view_element_mutations.py; \
     } > docs/evidence/E10/view-elements.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/view-elements.txt

# ADR-0073: a template reads each value by path. The checker's and the build's
# tests, the renderer's key tests, and the mutation controls.
e10-template-values:
    @{ echo "ADR-0073 - a template reads each value by path"; echo; \
       echo "produced by: just e10-template-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker, the template IR and the build (compiler/pw-core/tests/template_values.rs)"; echo; \
       cargo test --locked -p pw-core --test template_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer's keys (runtime/pw-render/tests/keys.rs)"; echo; \
       cargo test --locked -p pw-render --test keys 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/template_value_mutations.py)"; echo; \
       python3 scripts/template_value_mutations.py; \
     } > docs/evidence/E10/template-values.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/template-values.txt

# ADR-0074: what a template writes has a text form. Its tests, ADR-0073's
# (a computed list now checks, and does not build), and the mutation controls.
e10-template-text:
    @{ echo "ADR-0074 - what a template writes has a text form"; echo; \
       echo "produced by: just e10-template-text"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== what a template writes (compiler/pw-core/tests/template_text.rs)"; echo; \
       cargo test --locked -p pw-core --test template_text 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what a template reads by path (compiler/pw-core/tests/template_values.rs)"; echo; \
       cargo test --locked -p pw-core --test template_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/template_text_mutations.py)"; echo; \
       python3 scripts/template_text_mutations.py; \
     } > docs/evidence/E10/template-text.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/template-text.txt

# ADR-0075: a mounted resource does not build; since ADR-0148 a stream builds
# as a part. Its tests, and the mutation controls.
e10-streams:
    @{ echo "ADR-0075 - a mounted resource does not build; ADR-0148 - a stream builds as a part"; echo; \
       echo "produced by: just e10-streams"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== streams and resources (compiler/pw-core/tests/streams_and_resources.rs)"; echo; \
       cargo test --locked -p pw-core --test streams_and_resources 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/stream_mutations.py)"; echo; \
       python3 scripts/stream_mutations.py; \
     } > docs/evidence/E10/streams.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/streams.txt

# ADR-0076: an arm no value reaches is refused. Its tests, and the mutation
# controls.
e10-unreachable-arms:
    @{ echo "ADR-0076 - an arm no value reaches is refused"; echo; \
       echo "produced by: just e10-unreachable-arms"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== unreachable arms (compiler/pw-core/tests/unreachable_arms.rs)"; echo; \
       cargo test --locked -p pw-core --test unreachable_arms 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/unreachable_arm_mutations.py)"; echo; \
       python3 scripts/unreachable_arm_mutations.py; \
     } > docs/evidence/E10/unreachable-arms.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/unreachable-arms.txt

# ADR-0077: a call through a field holding a function is checked. Its tests,
# and the mutation controls.
e10-field-calls:
    @{ echo "ADR-0077 - a call through a field holding a function is checked"; echo; \
       echo "produced by: just e10-field-calls"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== field calls (compiler/pw-core/tests/field_calls.rs)"; echo; \
       cargo test --locked -p pw-core --test field_calls 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/field_call_mutations.py)"; echo; \
       python3 scripts/field_call_mutations.py; \
     } > docs/evidence/E10/field-calls.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/field-calls.txt

# ADR-0078: an effect is performed where its function is named. Its tests,
# the generality witnesses, and the mutation controls.
e10-effects-through-values:
    @{ echo "ADR-0078 - an effect is performed where its function is named"; echo; \
       echo "produced by: just e10-effects-through-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== effects through values (compiler/pw-core/tests/effects_through_values.rs)"; echo; \
       cargo test --locked -p pw-core --test effects_through_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the generality witnesses (compiler/pw-core/tests/generality.rs)"; echo; \
       cargo test --locked -p pw-core --test generality 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/effect_value_mutations.py)"; echo; \
       python3 scripts/effect_value_mutations.py; \
     } > docs/evidence/E10/effects-through-values.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/effects-through-values.txt

# ADR-0079: a function value carries the label of what it makes. Its tests,
# and the mutation controls.
e10-labels-through-values:
    @{ echo "ADR-0079 - a function value carries the label of what it makes"; echo; \
       echo "produced by: just e10-labels-through-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== labels through values (compiler/pw-core/tests/labels_through_values.rs)"; echo; \
       cargo test --locked -p pw-core --test labels_through_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/label_value_mutations.py)"; echo; \
       python3 scripts/label_value_mutations.py; \
     } > docs/evidence/E10/labels-through-values.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/labels-through-values.txt

# ADR-0080: the affine rule follows bindings, not names. Its tests, and the
# mutation controls.
e10-affine-bindings:
    @{ echo "ADR-0080 - the affine rule follows bindings, not names"; echo; \
       echo "produced by: just e10-affine-bindings"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== affine bindings (compiler/pw-core/tests/affine_bindings.rs)"; echo; \
       cargo test --locked -p pw-core --test affine_bindings 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/affine_binding_mutations.py)"; echo; \
       python3 scripts/affine_binding_mutations.py; \
     } > docs/evidence/E10/affine-bindings.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/affine-bindings.txt

# ADR-0081: a named argument is given to the parameter of its name. The
# checker's tests, the call run through the E8 host, and the mutation
# controls.
e10-named-arguments:
    @{ echo "ADR-0081 - a named argument is given to the parameter of its name"; echo; \
       echo "produced by: just e10-named-arguments"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker and the labels (compiler/pw-core/tests/named_arguments.rs)"; echo; \
       cargo test --locked -p pw-core --test named_arguments 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the call, through the E8 host (compiler/pw-conformance/tests/named_arguments.rs)"; echo; \
       cargo test --locked -p pw-conformance --test named_arguments 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/named_argument_mutations.py)"; echo; \
       python3 scripts/named_argument_mutations.py; \
     } > docs/evidence/E10/named-arguments.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/named-arguments.txt

# ADR-0082: a `derived` value performs no effect. Its tests, and the mutation
# controls.
e10-derived:
    @{ echo "ADR-0082 - a derived value performs no effect"; echo; \
       echo "produced by: just e10-derived"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== derived purity (compiler/pw-core/tests/derived_purity.rs)"; echo; \
       cargo test --locked -p pw-core --test derived_purity 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/derived_mutations.py)"; echo; \
       python3 scripts/derived_mutations.py; \
     } > docs/evidence/E10/derived.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/derived.txt

# ADR-0083: a call's arguments open on the callee's line. Its tests, the
# grammar's, and the mutation control.
e10-call-lines:
    @{ echo "ADR-0083 - a call's arguments open on the callee's line"; echo; \
       echo "produced by: just e10-call-lines"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== call lines (compiler/pw-core/tests/call_lines.rs)"; echo; \
       cargo test --locked -p pw-core --test call_lines 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the grammar (compiler/pw-syntax)"; echo; \
       cargo test --locked -p pw-syntax 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation control (scripts/call_line_mutations.py)"; echo; \
       python3 scripts/call_line_mutations.py; \
     } > docs/evidence/E10/call-lines.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/call-lines.txt

# ADR-0084: what a string interpolates has a text form. Its tests, and the
# mutation controls.
e10-string-holes:
    @{ echo "ADR-0084 - what a string interpolates has a text form"; echo; \
       echo "produced by: just e10-string-holes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== string holes (compiler/pw-core/tests/string_holes.rs)"; echo; \
       cargo test --locked -p pw-core --test string_holes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/string_hole_mutations.py)"; echo; \
       python3 scripts/string_hole_mutations.py; \
     } > docs/evidence/E10/string-holes.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/string-holes.txt

# ADR-0085: a call carries what it is given. Its tests, and the mutation
# controls.
e10-labels-through-plain-values:
    @{ echo "ADR-0085 - a call carries what it is given"; echo; \
       echo "produced by: just e10-labels-through-plain-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== labels through plain values (compiler/pw-core/tests/labels_through_plain_values.rs)"; echo; \
       cargo test --locked -p pw-core --test labels_through_plain_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/label_plain_mutations.py)"; echo; \
       python3 scripts/label_plain_mutations.py; \
     } > docs/evidence/E10/labels-through-plain-values.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/labels-through-plain-values.txt

# ADR-0086: a function crosses no boundary. Its test, and the mutation
# controls.
e10-function-captures:
    @{ echo "ADR-0086 - a function crosses no boundary"; echo; \
       echo "produced by: just e10-function-captures"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== function captures (compiler/pw-core/tests/function_captures.rs)"; echo; \
       cargo test --locked -p pw-core --test function_captures 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/function_capture_mutations.py)"; echo; \
       python3 scripts/function_capture_mutations.py; \
     } > docs/evidence/E10/function-captures.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/function-captures.txt

# ADR-0087: a call names a term. Its tests, and the mutation controls.
e10-call-names:
    @{ echo "ADR-0087 - a call names a term"; echo; \
       echo "produced by: just e10-call-names"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== call names (compiler/pw-core/tests/calls_name_terms.rs)"; echo; \
       cargo test --locked -p pw-core --test calls_name_terms 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/call_name_mutations.py)"; echo; \
       python3 scripts/call_name_mutations.py; \
     } > docs/evidence/E10/call-names.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/call-names.txt

# ADR-0088: a clause names a declaration of its kind, and gives it its key.
# Its tests, and the mutation controls.
e10-clause-keys:
    @{ echo "ADR-0088 - a clause names a declaration of its kind, and gives it its key"; echo; \
       echo "produced by: just e10-clause-keys"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== clause keys (compiler/pw-core/tests/clause_keys.rs)"; echo; \
       cargo test --locked -p pw-core --test clause_keys 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/clause_key_mutations.py)"; echo; \
       python3 scripts/clause_key_mutations.py; \
     } > docs/evidence/E10/clause-keys.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/clause-keys.txt

# ADR-0089: a policy's value is one its domain has. Its tests, and the
# mutation controls.
e10-policy-values:
    @{ echo "ADR-0089 - a policy's value is one its domain has"; echo; \
       echo "produced by: just e10-policy-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== policy values (compiler/pw-core/tests/policy_values.rs)"; echo; \
       cargo test --locked -p pw-core --test policy_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/policy_value_mutations.py)"; echo; \
       python3 scripts/policy_value_mutations.py; \
     } > docs/evidence/E10/policy-values.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/policy-values.txt

# ADR-0090: `pw build` checks what `pw check` checks. Its tests, the corpus
# standard the declaration rules are held to, and the mutation controls.
e10-one-checker:
    @{ echo "ADR-0090 - pw build checks what pw check checks"; echo; \
       echo "produced by: just e10-one-checker"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== one checker (compiler/pw-core/tests/one_checker.rs)"; echo; \
       cargo test --locked -p pw-core --test one_checker 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus standard (compiler/pw-core/tests/checking_source.rs)"; echo; \
       cargo test --locked -p pw-core --test checking_source 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/one_checker_mutations.py)"; echo; \
       python3 scripts/one_checker_mutations.py; \
     } > docs/evidence/E10/one-checker.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/one-checker.txt

# ADR-0091: a listener binds its entry's key. The checker's tests, the
# materializer's, and the mutation controls.
e10-listeners:
    @{ echo "ADR-0091 - a listener binds its entry's key"; echo; \
       echo "produced by: just e10-listeners"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== listener keys (compiler/pw-core/tests/listener_keys.rs)"; echo; \
       cargo test --locked -p pw-core --test listener_keys 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the materializer (runtime/pw-materialize/tests/listeners.rs)"; echo; \
       cargo test --locked -p pw-materialize --test listeners 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/listener_mutations.py)"; echo; \
       python3 scripts/listener_mutations.py; \
     } > docs/evidence/E10/listeners.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/listeners.txt

# ADR-0092: a dependency-graph clause belongs to a declaration that can mean
# it. Its tests, and the mutation controls.
e10-clause-places:
    @{ echo "ADR-0092 - a dependency-graph clause belongs to a declaration that can mean it"; echo; \
       echo "produced by: just e10-clause-places"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== clause places (compiler/pw-core/tests/clause_places.rs)"; echo; \
       cargo test --locked -p pw-core --test clause_places 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/clause_place_mutations.py)"; echo; \
       python3 scripts/clause_place_mutations.py; \
     } > docs/evidence/E10/clause-places.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/clause-places.txt

# ADR-0093: an element handles an event the platform declares. Its tests,
# and the mutation controls.
e10-declared-events:
    @{ echo "ADR-0093 - an element handles an event the platform declares"; echo; \
       echo "produced by: just e10-declared-events"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== declared events (compiler/pw-core/tests/declared_events.rs)"; echo; \
       cargo test --locked -p pw-core --test declared_events 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/declared_event_mutations.py)"; echo; \
       python3 scripts/declared_event_mutations.py; \
     } > docs/evidence/E10/declared-events.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/declared-events.txt

# ADR-0094: a template writes no code. Its tests, and the mutation controls.
e10-code-in-markup:
    @{ echo "ADR-0094 - a template writes no code"; echo; \
       echo "produced by: just e10-code-in-markup"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== code in markup (compiler/pw-core/tests/code_in_markup.rs)"; echo; \
       cargo test --locked -p pw-core --test code_in_markup 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/code_in_markup_mutations.py)"; echo; \
       python3 scripts/code_in_markup_mutations.py; \
     } > docs/evidence/E10/code-in-markup.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/code-in-markup.txt

# ADR-0095: an attribute's context is read as HTML reads its name. Its
# tests, and the mutation controls.
e10-attribute-case:
    @{ echo "ADR-0095 - an attribute's context is read as HTML reads its name"; echo; \
       echo "produced by: just e10-attribute-case"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== attribute case (compiler/pw-core/tests/attribute_case.rs)"; echo; \
       cargo test --locked -p pw-core --test attribute_case 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/attribute_case_mutations.py)"; echo; \
       python3 scripts/attribute_case_mutations.py; \
     } > docs/evidence/E10/attribute-case.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/attribute-case.txt

# ADR-0096: a template moves no URL. Its tests, and the mutation controls.
e10-moved-urls:
    @{ echo "ADR-0096 - a template moves no URL"; echo; \
       echo "produced by: just e10-moved-urls"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== moved URLs (compiler/pw-core/tests/platform_markup.rs)"; echo; \
       cargo test --locked -p pw-core --test platform_markup 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/moved_url_mutations.py)"; echo; \
       python3 scripts/moved_url_mutations.py; \
     } > docs/evidence/E10/moved-urls.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/moved-urls.txt

# ADR-0097: data embedded in a page cannot end its script element. The
# renderer's security matrix, and the mutation controls.
e10-embedded-json:
    @{ echo "ADR-0097 - data embedded in a page cannot end its script element"; echo; \
       echo "produced by: just e10-embedded-json"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the escaping matrix (runtime/pw-render/tests/security.rs)"; echo; \
       cargo test --locked -p pw-render --test security 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/embedded_json_mutations.py)"; echo; \
       python3 scripts/embedded_json_mutations.py; \
     } > docs/evidence/E10/embedded-json.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/embedded-json.txt

# ADR-0098: a name is written once where it is declared. Its tests, and the
# mutation controls.
e10-declared-once:
    @{ echo "ADR-0098 - a name is written once where it is declared"; echo; \
       echo "produced by: just e10-declared-once"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== declared once (compiler/pw-core/tests/declared_once.rs)"; echo; \
       cargo test --locked -p pw-core --test declared_once 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/declared_once_mutations.py)"; echo; \
       python3 scripts/declared_once_mutations.py; \
     } > docs/evidence/E10/declared-once.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/declared-once.txt

# ADR-0099: a failure is handled. Its tests, and the mutation controls.
e10-results-handled:
    @{ echo "ADR-0099 - a failure is handled"; echo; \
       echo "produced by: just e10-results-handled"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== results handled (compiler/pw-core/tests/results_handled.rs)"; echo; \
       cargo test --locked -p pw-core --test results_handled 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/results_handled_mutations.py)"; echo; \
       python3 scripts/results_handled_mutations.py; \
     } > docs/evidence/E10/results-handled.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/results-handled.txt

# ADR-0100: a query reads. Its tests, and the mutation controls.
e10-query-reads:
    @{ echo "ADR-0100 - a query reads"; echo; \
       echo "produced by: just e10-query-reads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== query reads (compiler/pw-core/tests/query_reads.rs, effects.rs)"; echo; \
       cargo test --locked -p pw-core --test query_reads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --lib a_query_may_read_and_not_write 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/query_read_mutations.py)"; echo; \
       python3 scripts/query_read_mutations.py; \
     } > docs/evidence/E10/query-reads.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/query-reads.txt

# ADR-0101: a command invalidates what it writes. Its tests, and the
# mutation controls.
e10-writes-invalidated:
    @{ echo "ADR-0101 - a command invalidates what it writes"; echo; \
       echo "produced by: just e10-writes-invalidated"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== writes invalidated (compiler/pw-core/tests/writes_invalidated.rs, effects.rs)"; echo; \
       cargo test --locked -p pw-core --test writes_invalidated 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --lib a_database_effect_names_its_domain 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/write_invalidation_mutations.py)"; echo; \
       python3 scripts/write_invalidation_mutations.py; \
     } > docs/evidence/E10/writes-invalidated.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/writes-invalidated.txt

# ADR-0102: an event reaches what reads what it invalidates. The
# materializer's tests, ADR-0091's listener tests it must keep passing, and
# the mutation controls.
e10-read-through:
    @{ echo "ADR-0102 - an event reaches what reads what it invalidates"; echo; \
       echo "produced by: just e10-read-through"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== reads followed (runtime/pw-materialize/tests/reads.rs)"; echo; \
       cargo test --locked -p pw-materialize --test reads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the M6 gate and the listeners, unchanged"; echo; \
       cargo test --locked -p pw-materialize --test gate --test listeners 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/read_through_mutations.py)"; echo; \
       python3 scripts/read_through_mutations.py; \
     } > docs/evidence/E10/read-through.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/read-through.txt

# ADR-0103: a write reaches the fragments built on it. Its tests, ADR-0101's
# it must keep passing, and the mutation controls.
e10-fragments-reached:
    @{ echo "ADR-0103 - a write reaches the fragments built on it"; echo; \
       echo "produced by: just e10-fragments-reached"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== fragments reached (compiler/pw-core/tests/writes_reach_fragments.rs)"; echo; \
       cargo test --locked -p pw-core --test writes_reach_fragments 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== ADR-0101's readers, unchanged"; echo; \
       cargo test --locked -p pw-core --test writes_invalidated 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/fragment_reach_mutations.py)"; echo; \
       python3 scripts/fragment_reach_mutations.py; \
     } > docs/evidence/E10/fragments-reached.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/fragments-reached.txt

# ADR-0104: the dev server commits the events a command declares. The
# server's tests, and the mutation controls.
e10-committed-events:
    @{ echo "ADR-0104 - the dev server commits the events a command declares"; echo; \
       echo "produced by: just e10-committed-events"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the dev server (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/committed_events_mutations.py)"; echo; \
       python3 scripts/committed_events_mutations.py; \
     } > docs/evidence/E10/committed-events.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/committed-events.txt

# ADR-0105: a command invalidates the entry it speculates on. Its tests,
# ADR-0101's and ADR-0103's it must keep passing, and the mutation controls.
e10-speculation-reconciled:
    @{ echo "ADR-0105 - a command invalidates the entry it speculates on"; echo; \
       echo "produced by: just e10-speculation-reconciled"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== speculations reconciled (compiler/pw-core/tests/speculation_reconciled.rs)"; echo; \
       cargo test --locked -p pw-core --test speculation_reconciled 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== ADR-0101's readers and ADR-0103's fragments, unchanged"; echo; \
       cargo test --locked -p pw-core --test writes_invalidated --test writes_reach_fragments 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/speculation_mutations.py)"; echo; \
       python3 scripts/speculation_mutations.py; \
     } > docs/evidence/E10/speculation-reconciled.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/speculation-reconciled.txt

# ADR-0106: `pw check` reports each file by its place. Its tests, run
# against the binary, and the mutation controls.
e10-same-name:
    @{ echo "ADR-0106 - pw check reports each file by its place"; echo; \
       echo "produced by: just e10-same-name"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== two files of one name (compiler/pw-cli/tests/same_name.rs)"; echo; \
       cargo test --locked -p pw-cli --test same_name 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/same_name_mutations.py)"; echo; \
       python3 scripts/same_name_mutations.py; \
     } > docs/evidence/E10/same-name.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/same-name.txt

# ADR-0107: a cache key names each parameter its entry depends on. Its
# tests, and the mutation controls.
e10-keys-cover-reads:
    @{ echo "ADR-0107 - a cache key names each parameter its entry depends on"; echo; \
       echo "produced by: just e10-keys-cover-reads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== keys cover reads (compiler/pw-core/tests/keys_cover_reads.rs)"; echo; \
       cargo test --locked -p pw-core --test keys_cover_reads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/key_read_mutations.py)"; echo; \
       python3 scripts/key_read_mutations.py; \
     } > docs/evidence/E10/keys-cover-reads.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/keys-cover-reads.txt

# ADR-0108: a query names a resource that exists. Its tests, and the
# mutation controls.
e10-queries-name-resources:
    @{ echo "ADR-0108 - a query names a resource that exists"; echo; \
       echo "produced by: just e10-queries-name-resources"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== queries name resources (compiler/pw-core/tests/queries_name_resources.rs)"; echo; \
       cargo test --locked -p pw-core --test queries_name_resources 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/query_name_mutations.py)"; echo; \
       python3 scripts/query_name_mutations.py; \
     } > docs/evidence/E10/queries-name-resources.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/queries-name-resources.txt

# ADR-0109: a timeout is a budget above zero. Its test, and the mutation
# controls.
e10-timeouts:
    @{ echo "ADR-0109 - a timeout is a budget above zero"; echo; \
       echo "produced by: just e10-timeouts"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== timeouts (compiler/pw-core/tests/timeouts.rs)"; echo; \
       cargo test --locked -p pw-core --test timeouts 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/timeout_mutations.py)"; echo; \
       python3 scripts/timeout_mutations.py; \
     } > docs/evidence/E10/timeouts.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/timeouts.txt

# ADR-0110: a resumable handler reads what it captures. Its tests, and the
# mutation controls.
e10-handler-captures:
    @{ echo "ADR-0110 - a resumable handler reads what it captures"; echo; \
       echo "produced by: just e10-handler-captures"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== handler captures (compiler/pw-core/tests/handler_captures.rs)"; echo; \
       cargo test --locked -p pw-core --test handler_captures 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/handler_capture_mutations.py)"; echo; \
       python3 scripts/handler_capture_mutations.py; \
     } > docs/evidence/E10/handler-captures.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/handler-captures.txt

# ADR-0111: a shorthand field reads its capture. Its tests, and the mutation
# controls.
e10-capture-shorthand:
    @{ echo "ADR-0111 - a shorthand field reads its capture"; echo; \
       echo "produced by: just e10-capture-shorthand"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== a shorthand field (compiler/pw-core/tests/capture_shorthand.rs)"; echo; \
       cargo test --locked -p pw-core --test capture_shorthand 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/capture_shorthand_mutations.py)"; echo; \
       python3 scripts/capture_shorthand_mutations.py; \
     } > docs/evidence/E10/capture-shorthand.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/capture-shorthand.txt

# ADR-0112: a call's privacy is the declaration it resolves to. Its tests,
# and the mutation controls.
e10-privacy-by-resolution:
    @{ echo "ADR-0112 - a call's privacy is the declaration it resolves to"; echo; \
       echo "produced by: just e10-privacy-by-resolution"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== privacy by resolution (compiler/pw-core/tests/privacy_by_resolution.rs)"; echo; \
       cargo test --locked -p pw-core --test privacy_by_resolution 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/privacy_resolution_mutations.py)"; echo; \
       python3 scripts/privacy_resolution_mutations.py; \
     } > docs/evidence/E10/privacy-by-resolution.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/privacy-by-resolution.txt

# ADR-0113: a resumable handler runs in the browser. The test, and the
# mutation controls.
e10-handlers-in-the-browser:
    @{ echo "ADR-0113 - a resumable handler runs in the browser"; echo; \
       echo "produced by: just e10-handlers-in-the-browser"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== handlers in the browser (compiler/pw-core/tests/handlers_in_the_browser.rs)"; echo; \
       cargo test --locked -p pw-core --test handlers_in_the_browser 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/handler_placement_mutations.py)"; echo; \
       python3 scripts/handler_placement_mutations.py; \
     } > docs/evidence/E10/handlers-in-the-browser.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/handlers-in-the-browser.txt

# ADR-0114: what is built before any request reads no request's value. The
# test, and the mutation controls.
e10-built-pages:
    @{ echo "ADR-0114 - what is built before any request reads no request's value"; echo; \
       echo "produced by: just e10-built-pages"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== built pages (compiler/pw-core/tests/built_pages.rs)"; echo; \
       cargo test --locked -p pw-core --test built_pages 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/built_page_mutations.py)"; echo; \
       python3 scripts/built_page_mutations.py; \
     } > docs/evidence/E10/built-pages.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/built-pages.txt

# ADR-0118: what a declaration reads, it reads through what it calls. A
# public query reading the session through a helper or another query, held
# to PW5101, PW5004, PW5001 and the contract, and the mutation controls.
e10-reads-through-calls:
    @{ echo "ADR-0118 - what a declaration reads, it reads through what it calls"; echo; \
       echo "produced by: just e10-reads-through-calls"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== reads through calls (compiler/pw-core/tests/reads_through_calls.rs)"; echo; \
       cargo test --locked -p pw-core --test reads_through_calls 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/reads_through_calls_mutations.py)"; echo; \
       python3 scripts/reads_through_calls_mutations.py; \
     } > docs/evidence/E10/reads-through-calls.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/reads-through-calls.txt

# ADR-0128: a shared cache holds no one reader's value, whatever its
# declaration says. The regression tests, the generality score they moved
# (31 / 31), and a mutant per piece of the decision.
e14-shared-cache:
    @{ echo "ADR-0128 - a shared cache holds no one reader's value, whatever its declaration says"; echo; \
       echo "produced by: just e14-shared-cache"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== regression tests (compiler/pw-core/tests/shared_cache_holds_no_readers_value.rs)"; echo; \
       cargo test --locked -p pw-core --test shared_cache_holds_no_readers_value 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== generality (just generality)"; echo; \
       cargo test --quiet --locked -p pw-core --test generality -- --nocapture 2>&1 | grep -E '^  (corpus|generally|narrowly|generality)'; \
       echo; echo "== mutation controls (scripts/shared_cache_mutations.py)"; echo; \
       python3 scripts/shared_cache_mutations.py; \
     } > docs/evidence/E14/shared-cache.txt
    @grep -E "^test result|mutants killed|generality|^---- |panicked at" docs/evidence/E14/shared-cache.txt

# ADR-0129: a value's label follows it through calls, bodies, branches and
# assignments, and a public log takes only a public value. The regression
# tests and a mutant per piece of the decision.
e14-value-labels:
    @{ echo "ADR-0129 - a value's label follows it through calls, bodies, branches and assignments"; echo; \
       echo "produced by: just e14-value-labels"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== regression tests (compiler/pw-core/tests/labels_follow_values.rs)"; echo; \
       cargo test --locked -p pw-core --test labels_follow_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/value_labels_mutations.py)"; echo; \
       python3 scripts/value_labels_mutations.py; \
     } > docs/evidence/E14/value-labels.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/value-labels.txt

# Its suite runs on the build it makes first, as every recipe's does (track
# evidence-integrity: it ran on whatever a worktree or a shard had built).
# ADR-0132: the browser's resume decision knows what the build compiled.
e14-resume-gate:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @{ echo "ADR-0132 - the browser's resume decision knows what the build compiled"; echo; \
       echo "produced by: just e14-resume-gate"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the decision (runtime/pw-resume-wasm)"; echo; \
       cargo test --locked -p pw-resume-wasm 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser, each engine (e2e/lazy-handler.spec.mjs)"; echo; \
       for e in webkit chromium; do \
         bash scripts/run_suite.sh -- pnpm exec playwright test e2e/lazy-handler.spec.mjs --project=$e --reporter=list; \
       done; \
       echo; echo "== mutation controls (scripts/resume_gate_mutations.py)"; echo; \
       python3 scripts/resume_gate_mutations.py; \
     } > docs/evidence/E14/resume-gate.txt
    @grep -E "^test result|passed|failed|mutants killed|^---- |panicked at" docs/evidence/E14/resume-gate.txt

# Its suite runs on the build it makes first (track evidence-integrity: it
# ran on whatever a worktree or a shard had built).
# ADR-0133: a page holds its own UI state, the first slice of signals. The
# compiler's tests, the browser's renderer, the page in each engine, and a
# mutant per piece.
e14-signals:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @{ echo "ADR-0133 - a page holds its own UI state (signals, first slice)"; echo; \
       echo "produced by: just e14-signals"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/signals.rs)"; echo; \
       cargo test --locked -p pw-core --test signals 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser's renderer (runtime/pw-render-wasm)"; echo; \
       cargo test --locked -p pw-render-wasm 2>&1 | grep -E '^(test |test result)|panicked at'; \
       ls -l target/wasm32-unknown-unknown/browser/pw_render_wasm.wasm 2>/dev/null | awk '{print "   pw-render.wasm: " $5 " bytes"}'; \
       echo; echo "== the page, each engine (e2e/signals.spec.mjs)"; echo; \
       for e in webkit chromium; do \
         bash scripts/run_suite.sh -- pnpm exec playwright test e2e/signals.spec.mjs --project=$e --reporter=list; \
       done; \
       echo; echo "== mutation controls (scripts/signals_mutations.py)"; echo; \
       python3 scripts/signals_mutations.py; \
     } > docs/evidence/E14/signals.txt
    @grep -E "^test result|passed|failed|mutants killed|^---- |panicked at" docs/evidence/E14/signals.txt

# ADR-0134: every handler is resumable, and what it captures is what it reads.
e14-handlers-resumable:
    @{ echo "ADR-0134 - every handler is resumable, and what it captures is what it reads"; echo; \
       echo "produced by: just e14-handlers-resumable"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/every_handler_is_resumable.rs)"; echo; \
       cargo test --locked -p pw-core --test every_handler_is_resumable 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/handlers_resumable_mutations.py)"; echo; \
       python3 scripts/handlers_resumable_mutations.py; \
     } > docs/evidence/E14/handlers-resumable.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/handlers-resumable.txt

# ADR-0135: a handler's identity is its own file's.
e14-handlers-by-file:
    @{ echo "ADR-0135 - a handler's identity is its own file's"; echo; \
       echo "produced by: just e14-handlers-by-file"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the regression (compiler/pw-core/tests/handlers_by_file.rs)"; echo; \
       cargo test --locked -p pw-core --test handlers_by_file 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/handlers_by_file_mutations.py)"; echo; \
       python3 scripts/handlers_by_file_mutations.py; \
     } > docs/evidence/E14/handlers-by-file.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/handlers-by-file.txt

# ADR-0136: a view used in another is written where it is used. The
# compiler's composition and the checks made where a view is used, the
# renderer reading a capture where the page holds it, and the mutation
# controls. The browser half is `e2e/views.spec.mjs`, in the own-renderer
# suite (`spikes/own-renderer/run.sh`).
e14-views-compose:
    @{ echo "ADR-0136 - a view is written where it is used"; echo; \
       echo "produced by: just e14-views-compose"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== composition (compiler/pw-core/tests/views_compose.rs)"; echo; \
       cargo test --locked -p pw-core --test views_compose 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a view's props (compiler/pw-core/tests/view_elements.rs)"; echo; \
       cargo test --locked -p pw-core --test view_elements 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a capture read where the page holds it (runtime/pw-render/tests/properties.rs)"; echo; \
       cargo test --locked -p pw-render --test properties capture 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/views_compose_mutations.py)"; echo; \
       python3 scripts/views_compose_mutations.py; \
     } > docs/evidence/E14/views-compose.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/views-compose.txt

# ADR-0137: a part a signal decides is one the browser renders again, and a
# block the browser renders reads what it holds. The plan's refusals, with
# controls, and the mutation controls.
e14-signals-render-again:
    @{ echo "ADR-0137 - what the browser renders again, it can"; echo; \
       echo "produced by: just e14-signals-render-again"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the plan's refusals (compiler/pw-core/tests/signals_render_again.rs)"; echo; \
       cargo test --locked -p pw-core --test signals_render_again 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/signals_render_again_mutations.py)"; echo; \
       python3 scripts/signals_render_again_mutations.py; \
     } > docs/evidence/E14/signals-render-again.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/signals-render-again.txt

# ADR-0138: a handler is given its event. The compiler's checks and the
# handler module run under Node, the grammar's, the renderer's one capture
# attribute per element, and the mutation controls. The browser half is
# `e2e/events.spec.mjs`, in the own-renderer suite.
e14-events:
    @{ echo "ADR-0138 - a handler is given its event"; echo; \
       echo "produced by: just e14-events"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/events.rs)"; echo; \
       cargo test --locked -p pw-core --test events 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the grammar (compiler/pw-syntax)"; echo; \
       cargo test --locked -p pw-syntax --lib grammar::tests::an_ 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== one capture attribute per element (runtime/pw-render/tests/properties.rs)"; echo; \
       cargo test --locked -p pw-render --test properties handlers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/events_mutations.py)"; echo; \
       python3 scripts/events_mutations.py; \
     } > docs/evidence/E14/events.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/events.txt

# The keyed-list suite below needs the build `spikes/own-renderer/run.sh`
# stages, which it makes first (track evidence-integrity: it ran on whatever
# a worktree or a shard had built).
# ADR-0139: a frame is forgotten when the page says it applied it. The
# server's tests, the mutation controls, and the keyed-list suite twenty
# times in a row, which failed intermittently before.
e14-stream-ack:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @{ echo "ADR-0139 - a frame is forgotten when the page says it applied it"; echo; \
       echo "produced by: just e14-stream-ack"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the server (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server stream 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/stream_ack_mutations.py)"; echo; \
       python3 scripts/stream_ack_mutations.py; \
       echo; echo "== the keyed-list suite, twenty runs"; echo; \
       cargo build --quiet --locked -p pw-dev-server; \
       bash scripts/run_suite.sh --runs 20 -- npx playwright test e2e/keyed-list.spec.mjs --reporter=line; \
     } > docs/evidence/E14/stream-ack.txt
    @grep -E "^test result|mutants killed|failed|^---- |panicked at" docs/evidence/E14/stream-ack.txt || true

# ADR-0140: a page that reads queries holds signals too. The server's tests
# and the mutation controls.
e14-store-signals:
    @{ echo "ADR-0140 - a page that reads queries holds signals too"; echo; \
       echo "produced by: just e14-store-signals"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the server (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server the_store 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/store_signals_mutations.py)"; echo; \
       python3 scripts/store_signals_mutations.py; \
     } > docs/evidence/E14/store-signals.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/store-signals.txt

# ADR-0141: a dialog a signal shows is the browser's modal dialog. The
# compiler's rule and the mutation controls; what the browser does with it
# is `e2e/dialog.spec.mjs`, in the own-renderer suite.
e14-dialogs:
    @{ echo "ADR-0141 - a dialog a signal shows is the browser's modal dialog"; echo; \
       echo "produced by: just e14-dialogs"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/dialogs.rs)"; echo; \
       cargo test --locked -p pw-core --test dialogs 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/dialogs_mutations.py)"; echo; \
       python3 scripts/dialogs_mutations.py; \
     } > docs/evidence/E14/dialogs.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/dialogs.txt

# ADR-0142: an input bound to a signal. The compiler's lowering, its rule
# and the plan's in-place parts, and the mutation controls; the browser's
# half is `e2e/bind.spec.mjs`, in the own-renderer suite.
e14-bind:
    @{ echo "ADR-0142 - an input bound to a signal"; echo; \
       echo "produced by: just e14-bind"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the binding (compiler/pw-core/tests/bind.rs)"; echo; \
       cargo test --locked -p pw-core --test bind 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what the browser sets in place (compiler/pw-core/tests/signals_render_again.rs)"; echo; \
       cargo test --locked -p pw-core --test signals_render_again 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/bind_mutations.py)"; echo; \
       python3 scripts/bind_mutations.py; \
     } > docs/evidence/E14/bind.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/bind.txt

# ADR-0147: a query binding is the query's value, and a page that cannot be
# read is answered. The typer's tests, the server's, `pw-render --plan`'s, and
# the mutation controls.
e14-query-values:
    @{ echo "ADR-0147 - a query binding is the query's value, and a page that cannot be read is answered"; echo; \
       echo "produced by: just e14-query-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the typer (compiler/pw-core/tests/query_values.rs)"; echo; \
       cargo test --locked -p pw-core --test query_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== pw-render --plan (runtime/pw-render/tests/plan_lists.rs)"; echo; \
       cargo test --locked -p pw-render --test plan_lists 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/query_values_mutations.py)"; echo; \
       python3 scripts/query_values_mutations.py; \
     } > docs/evidence/E14/query-values.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/query-values.txt

# ADR-0152: a key a page changes. A page's query given a signal is read again,
# for the new key, and its stale work is cancelled, superseded or kept as it
# says. The compiler's rules and plan, the server's keyed reads, presses run
# in the order they were made (three engines), and the mutation controls.
e14-keyed-reads:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/keyed-store.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0152 - a key a page changes, and what its stale work does"; echo; \
       echo "produced by: just e14-keyed-reads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the rules and the plan (compiler/pw-core/tests/keyed.rs)"; echo; \
       cargo test --locked -p pw-core --test keyed 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server's keyed reads, among its tests (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== keyed reads, the charter's store tests 6-8, and presses in order (e2e/keyed.spec.mjs, e2e/signals.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/keyed.spec.mjs e2e/signals.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/keyed_reads_mutations.py)"; echo; \
       python3 scripts/keyed_reads_mutations.py; \
       echo; echo "== the press-order control (scripts/press_order_mutations.py)"; echo; \
       python3 scripts/press_order_mutations.py; \
     } > docs/evidence/E14/keyed-reads.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/keyed-reads.txt

# ADR-0155: the page keeps its subscription through a dropped connection, and
# a press on a handler from another build reads the page again, once. The
# browser tests in three engines, and the mutation controls.
e14-runtime-recovery:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0155 - the page keeps its subscription, and recovers a refused handler"; echo; \
       echo "produced by: just e14-runtime-recovery"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the browser (e2e/transport.spec.mjs, e2e/recovery.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/transport.spec.mjs e2e/recovery.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/runtime_recovery_mutations.py)"; echo; \
       python3 scripts/runtime_recovery_mutations.py; \
     } > docs/evidence/E14/runtime-recovery.txt
    @grep -E "passed|mutants killed" docs/evidence/E14/runtime-recovery.txt

# ADR-0157: a handler is answered what its command did, `Ok` or the error it
# declares, never the value; and an item sold out since the page was rendered
# is refused by name. The rules, the compiled handlers, the oracle, the host,
# the server, the browser in three engines, and the mutation controls.
e14-command-answers:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0157 - what a command answers its handler, and an item sold out since the page"; echo; \
       echo "produced by: just e14-command-answers"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the rules, PW0339 and PW0620 (compiler/pw-core/tests/answers.rs)"; echo; \
       cargo test --locked -p pw-core --test answers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the compiled handlers, run under Node against each answer (compiler/pw-core/tests/handlers.rs)"; echo; \
       cargo test --locked -p pw-core --test handlers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the compiled command against its reference, each availability (compiler/pw-conformance/tests/oracle.rs)"; echo; \
       cargo test --locked -p pw-conformance --test oracle -- --nocapture 2>&1 | grep -E '^oracle: store|^test result|^---- |panicked at'; \
       echo; echo "== the compiled command in the host (runtime/pw-host/tests/pleris_component.rs)"; echo; \
       cargo test --locked -p pw-host --features engine --test pleris_component 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test (an_item_sold_out|a_kept_answer|tests::an_item_sold_out|tests::a_kept_answer)|^test result|^---- |panicked at'; \
       echo; echo "== the browser (e2e/availability.spec.mjs, e2e/resource-path.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/availability.spec.mjs e2e/resource-path.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/command_answers_mutations.py)"; echo; \
       python3 scripts/command_answers_mutations.py; \
     } > docs/evidence/E14/command-answers.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/command-answers.txt

# ADR-0159: a handler handles what its command answers. The rule, the
# discard by name compiled, the stores and the demos checked, and the mutation
# controls; every task's Pleris controls are `just e14-pleris-controls`.
e14-handler-failures:
    @cargo build --quiet --locked -p pw-cli
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0159 - a handler handles what its command answers"; echo; \
       echo "produced by: just e14-handler-failures"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/results_handled.rs)"; echo; \
       cargo test --locked -p pw-core --test results_handled 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the compiled handlers, a discard by name among them (compiler/pw-core/tests/handlers.rs)"; echo; \
       cargo test --locked -p pw-core --test handlers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the canonical store with its demos, the benchmark's store, kiokun"; echo; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw benchmarks/baselines/pleris/domain.pw benchmarks/baselines/pleris/lib/*.pw benchmarks/baselines/pleris/store/*.pw 2>&1 | tail -1; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | tail -1; \
       echo; echo "== mutation controls (scripts/handler_failures_mutations.py)"; echo; \
       python3 scripts/handler_failures_mutations.py; \
       echo; echo "== ADR-0099's, against the walk ADR-0159 rewrote (scripts/results_handled_mutations.py)"; echo; \
       python3 scripts/results_handled_mutations.py; \
     } > docs/evidence/E14/handler-failures.txt
    @grep -E "^test result|pw check|mutants killed|^---- |panicked at" docs/evidence/E14/handler-failures.txt

# ADR-0160: a page's route. The plan carries it, and the rules hold a route
# to its page's parameters (PW0340, PW0621) and to one page (PW0341).
e14-routes:
    @cargo build --quiet --locked -p pw-cli
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0160 - a page's route"; echo; \
       echo "produced by: just e14-routes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the tests (compiler/pw-core/tests/routes.rs)"; echo; \
       cargo test --locked -p pw-core --test routes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store declares its route, and checks"; echo; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       echo; echo "== mutation controls (scripts/routes_mutations.py)"; echo; \
       python3 scripts/routes_mutations.py; \
     } > docs/evidence/E14/routes.txt
    @grep -E "^test result|pw check|mutants killed|^---- |panicked at" docs/evidence/E14/routes.txt

# ADR-0161: each document is its own subscriber. The server's tests, two tabs
# of one session in three engines, and the mutation controls.
e14-documents:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/keyed-store.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0161 - each document is its own subscriber"; echo; \
       echo "produced by: just e14-documents"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_change_waiting|a_change_reaches|a_session_is_forgotten|a_read_older)|^test result|^---- |panicked at'; \
       echo; echo "== two tabs of one session (e2e/tabs.spec.mjs), and the keyed reads (e2e/keyed.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/tabs.spec.mjs e2e/keyed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/documents_mutations.py)"; echo; \
       python3 scripts/documents_mutations.py; \
     } > docs/evidence/E14/documents.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/documents.txt

# ADR-0162: each store at its route. The server's routing and stores, the
# stores' browser test in three engines, and the mutation controls.
e14-stores:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0162 - each store at its route"; echo; \
       echo "produced by: just e14-stores"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_path_gives|each_store_is|a_change_to_one_stores)|^test result|^---- |panicked at'; \
       echo; echo "== each store at its route, and test 11 (e2e/stores.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stores.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/stores_mutations.py)"; echo; \
       python3 scripts/stores_mutations.py; \
     } > docs/evidence/E14/stores.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/stores.txt

# ADR-0163: a page says when it is absent. PW0342 and the plan's case, the
# development server's 404, a store that is not there in three engines, and
# the mutation controls.
e14-not-found:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0163 - a page says when it is absent"; echo; \
       echo "produced by: just e14-not-found"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the tests (compiler/pw-core/tests/not_found.rs)"; echo; \
       cargo test --locked -p pw-core --test not_found 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store declares it, and checks"; echo; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(each_store_is|a_store_that_is_not_there)|^test result|^---- |panicked at'; \
       echo; echo "== a store that is not there, three engines (e2e/stores.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stores.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/not_found_mutations.py)"; echo; \
       python3 scripts/not_found_mutations.py; \
     } > docs/evidence/E14/not-found.txt
    @grep -E "^test result|pw check|passed|mutants killed|^---- |panicked at" docs/evidence/E14/not-found.txt

# ADR-0164: a menu change drops what declares it, for its store. The server's
# tests, the store's graph, and the mutation controls.
e14-menu-changed:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0164 - a menu change drops what declares it, for its store"; echo; \
       echo "produced by: just e14-menu-changed"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the store's graph: what MenuChanged(id) invalidates"; echo; \
       cargo run --quiet --locked -p pw-cli -- emit-graph --plain examples/domain.pw examples/lib/*.pw examples/store/*.pw \
         | grep -E "MenuChanged"; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_menu_change_drops|a_change_to_one_stores_menu)|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/menu_changed_mutations.py)"; echo; \
       python3 scripts/menu_changed_mutations.py; \
     } > docs/evidence/E14/menu-changed.txt
    @grep -E "^test result|MenuChanged|mutants killed|^---- |panicked at" docs/evidence/E14/menu-changed.txt

# ADR-0165: the store's delivery estimate and recommendations. The store's
# streams, the server's tests, the slots in three engines, and the mutation
# controls.
e14-slots:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0165 - the store's delivery estimate and recommendations"; echo; \
       echo "produced by: just e14-slots"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the store's streams (compiler/pw-core/tests/stream_plan.rs)"; echo; \
       cargo test --locked -p pw-core --test stream_plan 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store checks"; echo; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(the_store_sends_its_slots|a_menu_change_drops_that_stores)|^test result|^---- |panicked at'; \
       echo; echo "== the slots, three engines (e2e/slots.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/slots.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/slots_mutations.py)"; echo; \
       python3 scripts/slots_mutations.py; \
     } > docs/evidence/E14/slots.txt
    @grep -E "^test result|pw check|passed|skipped|mutants killed|^---- |panicked at" docs/evidence/E14/slots.txt

# ADR-0166: the store and its items say what they are, and a host's answer is
# read through the program's own types. The engine's tests, the server's, the
# slots in three engines (WebKit's paint among them), and the mutation
# controls.
e14-descriptions:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0166 - the store and its items say what they are"; echo; \
       echo "produced by: just e14-descriptions"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== a host's answer, read through the declared type (runtime/pw-host/tests/host_answers.rs)"; echo; \
       cargo test --locked -p pw-host --features engine --test host_answers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store checks"; echo; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       echo; echo "== the development server (pw-dev-server), the benchmark's store among its tests"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::the_store_and_each_item|^test result|^---- |panicked at'; \
       echo; echo "== the slots, three engines, WebKit painting the store before them (e2e/slots.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/slots.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/descriptions_mutations.py)"; echo; \
       python3 scripts/descriptions_mutations.py; \
     } > docs/evidence/E14/descriptions.txt
    @grep -E "^test result|pw check|passed|skipped|mutants killed|^---- |panicked at" docs/evidence/E14/descriptions.txt

# ADR-0167: markup text is text, and a comment in markup is `<!-- -->`. The
# parser's tests, the checker's (PW5028), the corpus checking clean, and the
# mutation controls.
e14-markup-comments:
    @cargo build --quiet --locked -p pw-cli
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0167 - markup text is text, and a comment in markup is <!-- -->"; echo; \
       echo "produced by: just e14-markup-comments"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the parser (compiler/pw-syntax)"; echo; \
       cargo test --locked -p pw-syntax --lib -- a_slash_slash_in_markup an_html_comment_in_markup an_unquoted_attribute_value 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the checker and what a page sends (compiler/pw-core/tests/markup_comments.rs)"; echo; \
       cargo test --locked -p pw-core --test markup_comments 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus: each rejected fixture emits its own defect, and no other"; echo; \
       cargo test --locked -p pw-core --test checking_source 2>&1 | grep -E '^test (every_rejected|no_accepted)|^test result|^---- |panicked at'; \
       echo; echo "== the store checks"; echo; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       echo; echo "== mutation controls (scripts/markup_comments_mutations.py)"; echo; \
       python3 scripts/markup_comments_mutations.py; \
     } > docs/evidence/E14/markup-comments.txt
    @grep -E "^test result|pw check|mutants killed|^---- |panicked at" docs/evidence/E14/markup-comments.txt

# ADR-0168: a change reaches every part that reads it. The renderer's tests,
# the server's, the store's names in three engines, and the mutation controls.
e14-instance-changes:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0168 - a change reaches every part that reads it"; echo; \
       echo "produced by: just e14-instance-changes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== what changed in an instance (runtime/pw-render/tests/instance_changes.rs)"; echo; \
       cargo test --locked -p pw-render --test instance_changes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_rename_sets_every_part|a_list_moves_what_moved)|^test result|^---- |panicked at'; \
       echo; echo "== each Add named by its item, renamed with it; an instance in place not moved (three engines)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stores.spec.mjs e2e/keyed-list.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/instance_changes_mutations.py)"; echo; \
       python3 scripts/instance_changes_mutations.py; \
     } > docs/evidence/E14/instance-changes.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/instance-changes.txt

# ADR-0169: what a template reads through a member function, a host computes
# or the build refuses. The plan's tests, the renderer's, the server's, each
# price in three engines, the store's own build, and the mutation controls.
e14-row-reads:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0169 - what a template reads through a member function"; echo; \
       echo "produced by: just e14-row-reads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== planned or refused (compiler/pw-core/tests/row_reads.rs)"; echo; \
       cargo test --locked -p pw-core --test row_reads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a path read field by field (runtime/pw-render/tests/paths.rs)"; echo; \
       cargo test --locked -p pw-render --test paths 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's plan: the row's read, and its member compiled"; echo; \
       out="$(mktemp -d)"; ./target/debug/pw build --out "$out" \
         packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw \
         examples/lib/*.pw examples/store/*.pw | grep -E "domain.display|page  "; \
       python3 -c "import json,sys; print(json.dumps(json.load(open(sys.argv[1]))['rows']))" "$out/pages/store.page.StorePage~ui.json"; \
       rm -rf "$out"; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(each_menu_row_shows|an_inserted_item_arrives|a_menu_change_drops)|^test result|^---- |panicked at'; \
       echo; echo "== each price, and an inserted item's, in three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stores.spec.mjs e2e/store.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/row_reads_mutations.py)"; echo; \
       python3 scripts/row_reads_mutations.py; \
     } > docs/evidence/E14/row-reads.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/row-reads.txt

# ADR-0170: a loop over a list inside a query's value, and a part a
# speculation would not reach. The plan's tests, the renderer's, the
# server's, and the mutation controls.
e14-nested-lists:
    @cargo build --quiet --locked -p pw-cli
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0170 - a loop over a list inside a query's value"; echo; \
       echo "produced by: just e14-nested-lists"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the plan, and what a speculation would not reach (compiler/pw-core/tests/nested_lists.rs)"; echo; \
       cargo test --locked -p pw-core --test nested_lists 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a value computed for a row, read whole (runtime/pw-render/tests/paths.rs)"; echo; \
       cargo test --locked -p pw-render --test paths 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_list_inside_a_querys_value|a_block_)|^test result|^---- |panicked at'; \
       echo; echo "== the store checks"; echo; \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw 2>&1 | tail -1; \
       echo; echo "== mutation controls (scripts/nested_lists_mutations.py)"; echo; \
       python3 scripts/nested_lists_mutations.py; \
     } > docs/evidence/E14/nested-lists.txt
    @grep -E "^test result|pw check|mutants killed|^---- |panicked at" docs/evidence/E14/nested-lists.txt

# ADR-0171: an attribute at the top of the page that reads a query's value is
# set again when the value changes. The plan's tests, the server's, and the
# mutation controls.
e14-query-attributes:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0171 - an attribute that reads a query's value is set again"; echo; \
       echo "produced by: just e14-query-attributes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the plan (compiler/pw-core/tests/query_attributes.rs)"; echo; \
       cargo test --locked -p pw-core --test query_attributes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::an_attribute_that_reads_a_query|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/query_attributes_mutations.py)"; echo; \
       python3 scripts/query_attributes_mutations.py; \
     } > docs/evidence/E14/query-attributes.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/query-attributes.txt

# ADR-0172: the cart lists its lines, and a speculation reaches every part
# that reads it. The renderer's tests and its WebAssembly build's, the
# compiler's, the transitions run under Node, the host's, the server's, the
# cart in three engines, and the mutation controls.
e14-cart-lines:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0172 - the cart lists its lines, and a speculation reaches every part that reads it"; echo; \
       echo "produced by: just e14-cart-lines"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the renderer in the browser (runtime/pw-render-wasm)"; echo; \
       cargo test --locked -p pw-render-wasm 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== an instance's changes, captures set in place (runtime/pw-render)"; echo; \
       cargo test --locked -p pw-render --test instance_changes --test properties 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== regions, refusals, what a handler sends (compiler/pw-core)"; echo; \
       cargo test --locked -p pw-core --test nested_lists --test handlers --test every_handler_is_resumable --test views_compose 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the transitions under Node, and each command against its reference (compiler/pw-conformance)"; echo; \
       cargo test --locked -p pw-conformance --test speculation --test oracle 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a record and a list from a browser (runtime/pw-host)"; echo; \
       cargo test --locked -p pw-host --features engine --test pleris_component --test list_arguments 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_lines_steps|a_line_records|a_sessions_changes|a_speculating_page|the_compiled_command_carries)|^test result|^---- |panicked at'; \
       echo; echo "== the cart in three engines (e2e/cart.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/cart.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/cart_lines_mutations.py)"; echo; \
       python3 scripts/cart_lines_mutations.py; \
     } > docs/evidence/E14/cart-lines.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/cart-lines.txt

# ADR-0173: a command is sent again where no answer came, and a command that
# is retried is idempotent. The checker's and the compiler's tests, the
# retries in three engines, and the mutation controls.
e14-command-retry:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0173 - a command is sent again where no answer came"; echo; \
       echo "produced by: just e14-command-retry"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== a retried command is idempotent; what a handler passes (compiler/pw-core)"; echo; \
       cargo test --locked -p pw-core --test policy_values --test handlers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== sent again where no answer came, in three engines (e2e/retry.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/retry.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/command_retry_mutations.py)"; echo; \
       python3 scripts/command_retry_mutations.py; \
     } > docs/evidence/E14/command-retry.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/command-retry.txt

# ADR-0174: charter §15.5's store delay, cart delay and one-shot database
# error. The server's tests, the controls in three engines, and the mutation
# controls.
e14-test-controls:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0174 - a store delay, a cart delay, and a one-shot database error"; echo; \
       echo "produced by: just e14-test-controls"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_one_shot|a_delay_slows)|^test result|^---- |panicked at'; \
       echo; echo "== the controls, and a rolled-back command, in three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/controls.spec.mjs e2e/resource-path.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/test_controls_mutations.py)"; echo; \
       python3 scripts/test_controls_mutations.py; \
     } > docs/evidence/E14/test-controls.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/test-controls.txt

# ADR-0175: charter §15.5's one-shot network error and forced reconnect,
# made by the server. The server's tests over real connections, the page in
# three engines, and the mutation controls.
e14-connection-faults:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0175 - a network error and a forced reconnect, made by the server"; echo; \
       echo "produced by: just e14-connection-faults"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server, over real connections (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_dropped_command|a_forced_reconnect)|^test result|^---- |panicked at'; \
       echo; echo "== the page, in three engines (e2e/connections.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/connections.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/connection_faults_mutations.py)"; echo; \
       python3 scripts/connection_faults_mutations.py; \
     } > docs/evidence/E14/connection-faults.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/connection-faults.txt

# ADR-0176: a regeneration that fails sends nothing, and is tried again. The
# server's test, the page in three engines, and the mutation controls.
e14-materializer-failure:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0176 - a regeneration that fails sends nothing, and is tried again"; echo; \
       echo "produced by: just e14-materializer-failure"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::a_regeneration_that_fails|^test result|^---- |panicked at'; \
       echo; echo "== the page, in three engines (e2e/materializer.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/materializer.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/materializer_failure_mutations.py)"; echo; \
       python3 scripts/materializer_failure_mutations.py; \
     } > docs/evidence/E14/materializer-failure.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/materializer-failure.txt

# ADR-0177: a public read whose origin fails is answered with the last value
# kept (charter §15.6 test 18). The runtime's tests, the checker's, the
# server's, the page in three engines, and the mutation controls.
e14-last-known-good:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0177 - a public read whose origin fails is answered with the last value kept"; echo; \
       echo "produced by: just e14-last-known-good"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the query runtime (runtime/pw-resource/tests/last_known_good.rs)"; echo; \
       cargo test --locked -p pw-resource --test last_known_good 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== PW0343 (compiler/pw-core/src/rules.rs)"; echo; \
       cargo test --locked -p pw-core --lib -- rules::tests::a_last_known_good 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::a_failed_origin|^test result|^---- |panicked at'; \
       echo; echo "== the page, in three engines (e2e/controls.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/controls.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/last_known_good_mutations.py)"; echo; \
       python3 scripts/last_known_good_mutations.py; \
     } > docs/evidence/E14/last-known-good.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/last-known-good.txt

# ADR-0178: whether an item can be ordered is shown before the press, and a
# change to it reaches every page open (charter §15.1, §15.2). The renderer,
# the value analysis, the development server, the page in three engines, and
# the mutation controls.
e14-availability:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0178 - whether an item can be ordered is shown before the press, and a change to it reaches every page open"; echo; \
       echo "produced by: just e14-availability"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the renderer (runtime/pw-render/tests/instance_changes.rs)"; echo; \
       cargo test --locked -p pw-render --test instance_changes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the value analysis and the compiled handler (pw-core)"; echo; \
       cargo test --locked -p pw-core --test value_relations -- listeners the_store_program 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --test handlers the_stores_handlers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_sold_out|a_stock_change|an_untold_stock|a_document_read_behind|a_change_sends|an_insert_keeps|a_menu_changed_at_its_source|a_rename_sets|an_item_sold_out)|^test result|^---- |panicked at'; \
       echo; echo "== the page, in three engines (e2e/availability.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/availability.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/availability_mutations.py)"; echo; \
       python3 scripts/availability_mutations.py; \
     } > docs/evidence/E14/availability.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/availability.txt

# ADR-0179: an opaque type states its invariant, and every construction and
# every boundary holds it. The parser, the checker, the conformance oracle,
# the host, the server, the page in three engines, and the mutation controls.
e14-invariants:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0179 - an opaque type states its invariant, and every construction and every boundary holds it"; echo; \
       echo "produced by: just e14-invariants"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the parser (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib -- an_opaque_type_states_its_invariant 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the checker, PW0622 and PW0623 (compiler/pw-core/tests/invariants.rs)"; echo; \
       cargo test --locked -p pw-core --test invariants 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the conformance oracle, its values fitted to the contract"; echo; \
       cargo test --locked -p pw-conformance --test oracle 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host (runtime/pw-host/tests/bounded.rs, pleris_component.rs)"; echo; \
       cargo test --locked -p pw-host --features engine --test bounded 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-host --features engine --test pleris_component -- a_quantity_that_is_no a_data_layer_answer 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_quantity_that_is_no_positive_int|a_stored_line_of_nothing)|^test result|^---- |panicked at'; \
       echo; echo "== the page, in three engines (e2e/compiled-handler.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/compiled-handler.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/invariants_mutations.py)"; echo; \
       python3 scripts/invariants_mutations.py; \
     } > docs/evidence/E14/invariants.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/invariants.txt

# ADR-0310: a name means its binding, in every analysis. The checker's cases,
# pw-core's tests, and the mutation controls.
e14-name-bindings:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0310 - a name means its binding, in every analysis"; echo; \
       echo "produced by: just e14-name-bindings"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker's cases (compiler/pw-core/tests/names_mean_their_binding.rs)"; echo; \
       cargo test --locked -p pw-core --test names_mean_their_binding 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== pw-core's library and the corpus (checking_source, every_name_resolves)"; echo; \
       cargo test --locked -p pw-core --lib --test checking_source --test every_name_resolves 2>&1 | grep -E '^test result|panicked at'; \
       echo; echo "== mutation controls (scripts/name_binding_mutations.py)"; echo; \
       python3 scripts/name_binding_mutations.py; \
     } > docs/evidence/E14/name-bindings.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/name-bindings.txt

# ADR-0304: what a page shows by the clock is told when the clock passes it.
# The zones, the checker's cases, the host's clock and timers, and the
# mutation controls.
e14-clock:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0304 - what a page shows by the clock is told when the clock passes it"; echo; \
       echo "produced by: just e14-clock"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "tz database: $(cat /usr/share/zoneinfo/+VERSION 2>/dev/null || sed -n 's/^# version //p' /usr/share/zoneinfo/tzdata.zi 2>/dev/null || echo unknown)"; echo; \
       echo "== time zones, read from the host's own database (runtime/pw-time)"; echo; \
       cargo test --locked -p pw-time 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== where a clock compared may be kept (compiler/pw-core: effects, tests/clock_compared.rs)"; echo; \
       cargo test --locked -p pw-core --lib effects 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --test clock_compared 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host's clock, what a value holds until, and a page told then (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server -- clock:: a_page_is_told_when a_kept_value_past a_page_that_compares_no the_hosts_clock 2>&1 \
         | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/clock_mutations.py)"; echo; \
       python3 scripts/clock_mutations.py; \
     } > docs/evidence/E14/clock.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/clock.txt

# ADR-0180: a delivery estimate is a range, and says when it was made. The
# server's tests, the page in three engines, and the mutation controls.
e14-estimate-range:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0180 - a delivery estimate is a range, and says when it was made"; echo; \
       echo "produced by: just e14-estimate-range"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_delivery_estimate_is_a_range|the_store_sends_its_slots|a_session_s_estimate|a_failed_estimate)|^test result|^---- |panicked at'; \
       echo; echo "== the page, in three engines (e2e/slots.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/slots.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/estimate_range_mutations.py)"; echo; \
       python3 scripts/estimate_range_mutations.py; \
     } > docs/evidence/E14/estimate-range.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/estimate-range.txt

# ADR-0181: a menu grouped by its category, and a list inside a row changed
# where it is. The plan's, the renderer's and the server's tests, the pages in
# three engines, and the mutation controls.
e14-menu-categories:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0181 - a menu is grouped by its category, and a list inside a row is changed where it is"; echo; \
       echo "produced by: just e14-menu-categories"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the plan (compiler/pw-core/tests/row_reads.rs)"; echo; \
       cargo test --locked -p pw-core --test row_reads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer (runtime/pw-render/tests/nested_lists.rs, instance_changes.rs)"; echo; \
       cargo test --locked -p pw-render --test nested_lists --test instance_changes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_menu_is_grouped|a_rename_sets|a_stock_change_told|an_insert_and_what|a_change_sends_what)|^test result|^---- |panicked at'; \
       echo; echo "== the pages, in three engines (e2e/stores.spec.mjs, e2e/keyed-list.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stores.spec.mjs e2e/keyed-list.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/menu_categories_mutations.py)"; echo; \
       python3 scripts/menu_categories_mutations.py; \
     } > docs/evidence/E14/menu-categories.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/menu-categories.txt

# ADR-0182: keyboard and screen-reader semantics remain valid (charter §15.6
# test 14). The store's page in three engines, as served and after each kind
# of change, and the mutation controls.
e14-accessibility:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0182 - keyboard and screen-reader semantics remain valid"; echo; \
       echo "produced by: just e14-accessibility"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the page, in three engines (e2e/accessibility.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/accessibility.spec.mjs --reporter=list; \
       echo; echo "== mutation controls (scripts/accessibility_mutations.py)"; echo; \
       python3 scripts/accessibility_mutations.py; \
     } > docs/evidence/E14/accessibility.txt
    @grep -E "passed|mutants killed" docs/evidence/E14/accessibility.txt

# ADR-0183: a page states its title. The compiler's, the renderer's and the
# server's tests, the corpus at C9, the page in three engines, and the
# mutation controls.
e14-titles:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0183 - a page states its title"; echo; \
       echo "produced by: just e14-titles"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/titles.rs)"; echo; \
       cargo test --locked -p pw-core --test titles 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus at C9 (corpus-check, corpus_history.rs, generality.rs, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test corpus_history --test generality --test checking_source 2>&1 \
         | grep -E '^test (the_c9|the_pre_change|generality_is|every_rejected|every_caught|a_caught_file|every_diagnostic)|^test result|generality-tested|^---- |panicked at'; \
       echo; echo "== the renderer (runtime/pw-render/tests/titles.rs)"; echo; \
       cargo test --locked -p pw-render --test titles 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_store_s_page_is_titled|a_title_that_changed)|^test result|^---- |panicked at'; \
       echo; echo "== the page, in three engines (e2e/accessibility.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/accessibility.spec.mjs --reporter=list; \
       echo; echo "== mutation controls (scripts/titles_mutations.py)"; echo; \
       python3 scripts/titles_mutations.py; \
     } > docs/evidence/E14/titles.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/titles.txt

# ADR-0184: what a cache may keep holds nothing of a session's (charter
# §15.6 tests 2 and 13). The server's tests against the running store, the
# browser in three engines, and the mutation controls.
e14-shared-output:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0184 - what a cache may keep holds nothing of a session's"; echo; \
       echo "produced by: just e14-shared-output"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server, running the store (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_session_s_response_is_kept|what_a_shared_cache_keeps)|^test result|^---- |panicked at'; \
       echo; echo "== the page, in three engines (e2e/shared-output.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/shared-output.spec.mjs --reporter=list; \
       echo; echo "== mutation controls (scripts/shared_output_mutations.py)"; echo; \
       python3 scripts/shared_output_mutations.py; \
     } > docs/evidence/E14/shared-output.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/shared-output.txt

# ADR-0185: ids, and the ARIA that names them, checked at build (charter
# §8.2). The compiler's tests, the corpus at C10, every program clean, and the
# mutation controls.
e14-ids:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0185 - ids, and the ARIA that names them, checked at build"; echo; \
       echo "produced by: just e14-ids"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rules (compiler/pw-core/tests/ids_and_aria.rs, labels.rs)"; echo; \
       cargo test --locked -p pw-core --test ids_and_aria --test labels 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus at C10 (corpus-check, corpus_history.rs, generality.rs, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test corpus_history --test generality --test checking_source 2>&1 \
         | grep -E '^test (the_c10|the_c9|the_pre_change|generality_is|every_rejected|every_caught|a_caught_file|every_diagnostic)|^test result|^---- |panicked at'; \
       echo; echo "== every program the repository checks"; echo; \
       python3 scripts/accepted_corpus.py; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         benchmarks/baselines/pleris/domain.pw benchmarks/baselines/pleris/lib/*.pw benchmarks/baselines/pleris/store/*.pw 2>&1 | tail -1; \
       echo; echo "== mutation controls (scripts/ids_mutations.py)"; echo; \
       python3 scripts/ids_mutations.py; \
     } > docs/evidence/E14/ids.txt
    @grep -E "^test result|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/ids.txt

# ADR-0189: a `<link>` is written where HTML allows it. The rule's tests, the
# corpus at C12, every program clean, and the mutation controls.
e14-links:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0189 - a <link> is written where HTML allows it"; echo; \
       echo "produced by: just e14-links"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/links.rs)"; echo; \
       cargo test --locked -p pw-core --test links 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus at C12 (corpus-check, generality.rs, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test generality --test checking_source 2>&1 \
         | grep -E '^test (generality_is|every_rejected|every_caught|a_caught_file|every_diagnostic)|^test result|^---- |panicked at'; \
       echo; echo "== every program the repository checks"; echo; \
       python3 scripts/accepted_corpus.py; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         benchmarks/baselines/pleris/domain.pw benchmarks/baselines/pleris/lib/*.pw benchmarks/baselines/pleris/store/*.pw 2>&1 | tail -1; \
       echo; echo "== mutation controls (scripts/links_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/links_mutations.py; \
     } > docs/evidence/E14/links.txt
    @grep -E "^test result|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/links.txt

# ADR-0190: every page that binds a query is served at its route and kept
# current by its own plan. The server's tests, the store's cart as a page of
# its own in three engines, and the mutation controls.
e14-pages:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0190 - every page that binds a query is served at its route"; echo; \
       echo "produced by: just e14-pages"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_page_that_binds_a_query|a_change_reaches_each_page|a_page_without_the_menu)|^test result|^---- |panicked at'; \
       echo; echo "== the cart's own page, in three engines (e2e/pages.spec.mjs, e2e/accessibility.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/pages.spec.mjs e2e/accessibility.spec.mjs \
          -g "cart|page of its own|reaches the other|links to the cart" --reporter=list; \
       echo; echo "== mutation controls (scripts/pages_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/pages_mutations.py; \
     } > docs/evidence/E14/pages.txt
    @grep -E "^test result|passed|failed|mutants killed|^---- |panicked at" docs/evidence/E14/pages.txt

# ADR-0191: every page speculates from its own module. The server's tests,
# the cart's page showing a press before the server answers in three
# engines, and the mutation controls.
e14-page-speculation:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0191 - every page speculates from its own module"; echo; \
       echo "produced by: just e14-page-speculation"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_page_that_binds_a_query|a_change_reaches_each_page)|^test result|^---- |panicked at'; \
       echo; echo "== the cart's page, in three engines (e2e/pages.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/pages.spec.mjs --reporter=list; \
       echo; echo "== mutation controls (scripts/page_speculation_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/page_speculation_mutations.py; \
     } > docs/evidence/E14/page-speculation.txt
    @grep -E "^test result|passed|failed|mutants killed|^---- |panicked at" docs/evidence/E14/page-speculation.txt

# ADR-0192: the stores, as the home page. The server's test, the home page
# in three engines with its audit, and the mutation controls.
e14-home:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0192 - the stores, as the home page"; echo; \
       echo "produced by: just e14-home"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::the_stores_are_the_home_page|^test result|^---- |panicked at'; \
       echo; echo "== the home page, in three engines (e2e/pages.spec.mjs, e2e/accessibility.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/pages.spec.mjs e2e/accessibility.spec.mjs \
          -g "home page|stores are the home" --reporter=list; \
       echo; echo "== mutation controls (scripts/home_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/home_mutations.py; \
     } > docs/evidence/E14/home.txt
    @grep -E "^test result|passed|failed|mutants killed|^---- |panicked at" docs/evidence/E14/home.txt

# ADR-0193: an order is placed from the cart, and its page follows it as the
# store moves it along. The server's tests, the order's flow in three
# engines with its audit, and the mutation controls.
e14-orders:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0193 - an order is placed, and its page follows it"; echo; \
       echo "produced by: just e14-orders"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(an_order_is_placed|an_empty_cart_places|the_store_moving_an_order)|^test result|^---- |panicked at'; \
       echo; echo "== the order's flow, in three engines (e2e/pages.spec.mjs, e2e/accessibility.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/pages.spec.mjs e2e/accessibility.spec.mjs -g "order" --reporter=list; \
       echo; echo "== mutation controls (scripts/orders_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/orders_mutations.py; \
     } > docs/evidence/E14/orders.txt
    @grep -E "^test result|passed|failed|mutants killed|^---- |panicked at" docs/evidence/E14/orders.txt

# ADR-0202: a type that holds itself in place is boxed, and crosses as its
# nodes. The WIT and the lowering, the components through the E8 host, the
# JavaScript modules, and the mutation controls.
e14-boxed-types:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0202 - a type that holds itself in place is boxed, and crosses as its nodes"; echo; \
       echo "produced by: just e14-boxed-types"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the WIT, the refusals and the lowering (compiler/pw-core/tests/recursive_types.rs)"; echo; \
       cargo test --locked -p pw-core --test recursive_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== components through the E8 host (compiler/pw-conformance/tests/boxed_types.rs, recursive_types.rs)"; echo; \
       cargo test --locked -p pw-conformance --test boxed_types -- --test-threads=1 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-conformance --test recursive_types -- --test-threads=1 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== the JavaScript modules, against their components under Node (compiler/pw-conformance/tests/javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test javascript a_type_that_holds_itself -- --nocapture 2>&1 | grep -E '^javascript:|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/boxed_types_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/boxed_types_mutations.py; \
     } > docs/evidence/E14/boxed-types.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E14/boxed-types.txt

# ADR-0203 (ADR-0130's ruling 2): a view that contains itself is an
# instance of its own template, made at run time. The compiler's, the
# renderer's and the in-browser renderer's tests, the thread page in three
# engines, and the mutation controls.
e14-view-instances:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0203 - a view that contains itself is an instance of its own template, made at run time"; echo; \
       echo "produced by: just e14-view-instances"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/views_contain_themselves.rs)"; echo; \
       cargo test --locked -p pw-core --test views_contain_themselves 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer (runtime/pw-render/tests/properties.rs) and the browser's (runtime/pw-render-wasm)"; echo; \
       cargo test --locked -p pw-render --test properties 2>&1 | grep -E '^test result|^---- |panicked at'; \
       cargo test --locked -p pw-render-wasm 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the thread page (spikes/own-renderer/e2e/thread.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/thread.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/view_instances_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/view_instances_mutations.py; \
     } > docs/evidence/E14/view-instances.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/view-instances.txt

# ADR-0205: a value of a type that contains itself crosses the browser's
# wire as its nodes. The renderer's, the modules', the host's and the
# component's tests, the thread page in three engines, and the controls.
e14-graphs-on-the-wire:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0205 - a value of a type that contains itself crosses the browser's wire as its nodes"; echo; \
       echo "produced by: just e14-graphs-on-the-wire"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the renderer (runtime/pw-render/tests/graphs.rs)"; echo; \
       cargo test --locked -p pw-render --test graphs --test graph_stack_probe 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser's modules, under Node (compiler/pw-core/tests/graphs_on_the_wire.rs, recursive_types.rs)"; echo; \
       cargo test --locked -p pw-core --test graphs_on_the_wire 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --test recursive_types 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== the host and a component (compiler/pw-conformance/tests/browser_graphs.rs)"; echo; \
       cargo test --locked -p pw-conformance --test browser_graphs 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the thread page (spikes/own-renderer/e2e/thread.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/thread.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/graphs_on_the_wire_mutations.py)"; echo; \
       python3 scripts/graphs_producer_mutations.py; \
       CARGO_INCREMENTAL=0 python3 scripts/graphs_on_the_wire_mutations.py; \
     } > docs/evidence/E14/graphs-on-the-wire.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/graphs-on-the-wire.txt

# ADR-0206: a case has one name where values are rendered, its WIT case's.
# Its tests, the renderer's and the hosts', and the mutation controls.
e14-one-case-name:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0206 - a case has one name where values are rendered, its WIT case's"; echo; \
       echo "produced by: just e14-one-case-name"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== a page matching on what its signals hold (compiler/pw-core/tests/one_name_for_a_case.rs)"; echo; \
       cargo test --locked -p pw-core --test one_name_for_a_case 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the template, the renderer and the hosts"; echo; \
       cargo test --locked -p pw-core --test template_blocks --test evidence_is_current 2>&1 | grep -E '^test result|^---- |panicked at'; \
       cargo test --locked -p pw-render 2>&1 | grep -E '^test result|^---- |panicked at'; \
       cargo test --locked -p pw-dev-server -p kiokun-server 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/one_case_name_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/one_case_name_mutations.py; \
     } > docs/evidence/E14/one-case-name.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/one-case-name.txt

# ADR-0225: a `String`'s length is an invariant. The compiler's, the host's
# and the server's tests, the feed in three engines, and the mutation
# controls.
e14-string-invariants:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0225 - a String's length is an invariant"; echo; \
       echo "produced by: just e14-string-invariants"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/string_invariants.rs, invariants.rs)"; echo; \
       cargo test --locked -p pw-core --test string_invariants --test invariants 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host (runtime/pw-host/tests/bounded.rs)"; echo; \
       cargo test --locked -p pw-host --features engine --test bounded 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_posts_text_is_held_to_its_length_where_it_arrives 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/string_invariants_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/string_invariants_mutations.py; \
     } > docs/evidence/E14/string-invariants.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/string-invariants.txt

# ADR-0226: a value the template computes compiles. The compiler's tests,
# the corpus at C16, the server's, the feed in three engines, and the
# mutation controls.
e14-computed-holes:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0226 - a value the template computes compiles"; echo; \
       echo "produced by: just e14-computed-holes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/computed_holes.rs, template_values.rs, effects_through_values.rs)"; echo; \
       cargo test --locked -p pw-core --test computed_holes --test template_values --test effects_through_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus at C16 (corpus-check, checking_source.rs, generality.rs, corpus_history.rs, rule_fixtures.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test checking_source --test generality --test corpus_history --test rule_fixtures 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- computed 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/computed_holes_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/computed_holes_mutations.py; \
     } > docs/evidence/E14/computed-holes.txt
    @grep -E "^test result|passed|corpus-check|mutants killed|^---- |panicked at" docs/evidence/E14/computed-holes.txt

# ADR-0227: a value computed from a signal is the browser's, and its first
# value the host's. The compiler's and the server's tests, the feed in three
# engines, and the mutation controls.
e14-computed-signals:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0227 - a value computed from a signal is the browser's, and its first value the host's"; echo; \
       echo "produced by: just e14-computed-signals"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/computed_signals.rs, computed_holes.rs)"; echo; \
       cargo test --locked -p pw-core --test computed_signals --test computed_holes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- from_a_signal signals_alone 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/computed_signals_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/computed_signals_mutations.py; \
     } > docs/evidence/E14/computed-signals.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/computed-signals.txt

# ADR-0228: a value computed in a row is the row's. The compiler's and the
# server's tests, the feed in three engines, and the mutation controls.
e14-computed-rows:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0228 - a value computed in a row is the row's"; echo; \
       echo "produced by: just e14-computed-rows"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/computed_rows.rs, computed_holes.rs, effects_through_values.rs, optimistic_keys.rs)"; echo; \
       cargo test --locked -p pw-core --test computed_rows --test computed_holes --test effects_through_values --test optimistic_keys 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- each_rows_and_sent compares_as_its_representation 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/computed_rows_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/computed_rows_mutations.py; \
     } > docs/evidence/E14/computed-rows.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/computed-rows.txt

# ADR-0229: a computed condition decides its block. The compiler's and the
# server's tests, the feed in three engines, and the mutation controls.
e14-computed-conditions:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0229 - a computed condition decides its block"; echo; \
       echo "produced by: just e14-computed-conditions"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/computed_conditions.rs, template_values.rs)"; echo; \
       cargo test --locked -p pw-core --test computed_conditions --test template_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_condition_ 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== native producer controls (scripts/computed_conditions_producer_mutations.py)"; echo; \
       python3 scripts/computed_conditions_producer_mutations.py; \
       echo; echo "== mutation controls (scripts/computed_conditions_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/computed_conditions_mutations.py; \
     } > docs/evidence/E14/computed-conditions.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/computed-conditions.txt

# ADR-0230: a condition is a `Bool`, or a `List` or `String` tested
# non-empty (ruling 0071-a). The checker's tests, the corpus at C17, and the
# mutation controls.
e14-condition-truth:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0230 - a condition is a Bool, or a List or String tested non-empty"; echo; \
       echo "produced by: just e14-condition-truth"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/template_truth.rs, template_operands.rs)"; echo; \
       cargo test --locked -p pw-core --test template_truth --test template_operands 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus at C17 (corpus-check, checking_source.rs, generality.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test checking_source --test generality 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/condition_truth_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/condition_truth_mutations.py; \
     } > docs/evidence/E14/condition-truth.txt
    @grep -E "^test result|corpus-check|mutants killed|^---- |panicked at" docs/evidence/E14/condition-truth.txt

# ADR-0224: a longer read is not applied over a commit it did not see. The
# server's tests and the mutation control.
e14-keyed-race:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0224 - a longer read is not applied over a commit it did not see"; echo; \
       echo "produced by: just e14-keyed-race"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_longer_read_is_not_applied_over_a_commit_it_did_not_see a_page_that_reads_more_holds_what_it_shows under_cancel_a_newer_key_stops_the_old_keys_read under_supersede_the_old_keys_read_runs_on_unshown 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/keyed_race_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/keyed_race_mutations.py; \
     } > docs/evidence/E14/keyed-race.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/keyed-race.txt

# ADR-0223: a streamed region is filled when its whole arm has arrived. The
# renderer's tests, the slots in three engines, and the mutation controls.
e14-whole-fills:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0223 - a streamed region is filled when its whole arm has arrived"; echo; \
       echo "produced by: just e14-whole-fills"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the patch and the comment after it (runtime/pw-render/tests/streams.rs)"; echo; \
       cargo test --locked -p pw-render --test streams 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the slots in three engines (e2e/slots.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/slots.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/whole_fills_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/whole_fills_mutations.py; \
     } > docs/evidence/E14/whole-fills.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/whole-fills.txt

# ADR-0222: a post is shown before the server answers. The compiler's and
# the server's tests, the feed in three engines, and the mutation controls.
e14-optimistic-posts:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0222 - a post is shown before the server answers"; echo; \
       echo "produced by: just e14-optimistic-posts"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== a key left unnamed (compiler/pw-core/tests/optimistic_keys.rs)"; echo; \
       cargo test --locked -p pw-core --test optimistic_keys 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_post_is_shown_before_the_server_answers a_speculated_value_that_did_not_change_is_not_sent_again a_page_that_reads_more_holds_what_it_shows a_speculating_page_is_sent_its_carts_value 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/optimistic_posts_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/optimistic_posts_mutations.py; \
     } > docs/evidence/E14/optimistic-posts.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/optimistic-posts.txt

# ADR-0275: a row shown before the server answers waits. The feed in three
# engines (e2e/feed.spec.mjs), and the mutation controls.
e14-waiting-rows:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0275 - a row shown before the server answers waits"; echo; \
       echo "produced by: just e14-waiting-rows"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/waiting_rows_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/waiting_rows_mutations.py; \
     } > docs/evidence/E14/waiting-rows.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/waiting-rows.txt

# ADR-0221: a form control's value is written where HTML reads it. The
# compiler's and the renderer's tests, the bound fields in three engines, the
# corpus, and the mutation controls.
e14-form-controls:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0221 - a form control's value is written where HTML reads it"; echo; \
       echo "produced by: just e14-form-controls"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the template and the check (compiler/pw-core/tests/form_controls.rs)"; echo; \
       cargo test --locked -p pw-core --test form_controls 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer's escaping matrix (runtime/pw-render/tests/security.rs)"; echo; \
       cargo test --locked -p pw-render --test security 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus (C15) and generality"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test generality 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== bound fields in three engines (e2e/bind.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/bind.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/form_controls_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/form_controls_mutations.py; \
     } > docs/evidence/E14/form-controls.txt
    @grep -E "^test result|passed|corpus-check|mutants killed|^---- |panicked at" docs/evidence/E14/form-controls.txt

# ADR-0220: the feed reference app, served in browsers by the host that
# serves the store. The server's tests, the feed in three engines, and the
# mutation controls.
e14-feed:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0220 - the feed reference app, served in browsers by the host that serves the store"; echo; \
       echo "produced by: just e14-feed"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- the_feeds_page_opening_its_stream_is_not_told_to_reload a_page_carries_its_data_layers_style_and_no_other a_guests_post_names_the_guest_to_every_reader 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/feed_served_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/feed_served_mutations.py; \
     } > docs/evidence/E14/feed.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/feed.txt

# ADR-0246: the feed's data in PostgreSQL, held to what its source states.
# Needs PW_FEED_DATABASE_URL, a throwaway database (each test uses a schema of
# its own and drops it), e.g. postgresql://localhost/pw_feed_test; skips
# without one, and writes no evidence.
e14-feed-postgres:
    @if [ -z "${PW_FEED_DATABASE_URL:-}" ]; then \
       echo "e14-feed-postgres: skipped, PW_FEED_DATABASE_URL is not set (a throwaway PostgreSQL database, e.g. postgresql://localhost/pw_feed_test)"; \
       exit 0; \
     fi; \
     mkdir -p docs/evidence/E14; \
     { echo "ADR-0246 - the feed's data in PostgreSQL, held to what its source states"; echo; \
       echo "produced by: just e14-feed-postgres"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$PW_FEED_DATABASE_URL" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== the feed on PostgreSQL (spikes/own-renderer/server/src/tests/feed_pg.rs, follows.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- tests::feed_pg:: tests::follows::on_postgres 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the in-memory layer, unchanged (the server's other tests)"; echo; \
       cargo test --locked -p pw-dev-server -- --skip tests::feed_pg:: --skip tests::follows::on_postgres 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/feed_postgres_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/feed_postgres_mutations.py; \
     } > docs/evidence/E14/feed-postgres.txt; \
     grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/feed-postgres.txt

# Track store-pg: the store's data behind the DataLayer seam on PostgreSQL,
# held to what its source states. Needs PW_STORE_DATABASE_URL, a throwaway
# database (each test uses a schema of its own, `pw_store_test_...`, and drops
# it); PW_FEED_DATABASE_URL stands in where it is not set, as on CI's
# database shard. Skips without either, and writes no evidence. Runs the
# server's whole suite twice: the store on PostgreSQL, and in memory.
e14-store-postgres:
    @url="${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}"; \
     if [ -z "$url" ]; then \
       echo "e14-store-postgres: skipped, PW_STORE_DATABASE_URL is not set (a throwaway PostgreSQL database, e.g. postgresql://localhost/pw_store_test)"; \
       exit 0; \
     fi; \
     export PW_STORE_DATABASE_URL="$url"; \
     mkdir -p docs/evidence/E14; \
     { echo "Track store-pg - the store's data on PostgreSQL, held to what its source states"; echo; \
       echo "produced by: just e14-store-postgres"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== the store on PostgreSQL (spikes/own-renderer/server/src/tests/store_pg.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- tests::store_pg:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server's whole suite, the store on PostgreSQL (PW_STORE_TEST_LAYER=postgres)"; echo; \
       PW_STORE_TEST_LAYER=postgres cargo test --locked -p pw-dev-server -- --skip tests::store_pg:: 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== the server's whole suite, the store in memory"; echo; \
       PW_STORE_TEST_LAYER=memory cargo test --locked -p pw-dev-server -- --skip tests::store_pg:: 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/store_postgres_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/store_postgres_mutations.py; \
     } > docs/evidence/E14/store-postgres.txt; \
     grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/store-postgres.txt

# ADR-0325: an interaction's answer commits with its writes (NEXT 5). The
# host's records in memory, the store's and the feed's on PostgreSQL, a host
# started after another, and two hosts at once; the materializer's own; and
# the mutation controls (scripts/interactions_mutations.py). Needs
# PW_STORE_DATABASE_URL, a throwaway database (each test uses a schema of its
# own and drops it), or PW_FEED_DATABASE_URL; skips without one.
e14-interactions:
    @url="${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}"; \
     if [ -z "$url" ]; then \
       echo "e14-interactions: skipped, PW_STORE_DATABASE_URL is not set (a throwaway PostgreSQL database, e.g. postgresql://localhost/pw_store_test)"; \
       exit 0; \
     fi; \
     export PW_STORE_DATABASE_URL="$url" PW_FEED_DATABASE_URL="$url"; \
     mkdir -p docs/evidence/E14; \
     { echo "ADR-0325 - an interaction's answer commits with its writes"; echo; \
       echo "produced by: just e14-interactions"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== the materializer's record (runtime/pw-materialize)"; echo; \
       cargo test --locked -p pw-materialize 2>&1 | grep -E '^test result|panicked at'; \
       echo; echo "== the host's records, in memory, and on PostgreSQL (store_pg.rs, feed_pg.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- interactions_kept_are_bounded_per_session an_interactions_record_answers_it_for_a_day on_postgres_an_interactions_answer_outlives_its_host on_postgres_a_second_hosts_commit_answers_what_the_first_committed on_postgres_an_interactions_record_answers_a_day on_postgres_a_retried_interaction_runs_its_command_once on_postgres_a_posts_interaction_outlives_its_host 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/interactions_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/interactions_mutations.py; \
     } > docs/evidence/E14/interactions.txt; \
     grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/interactions.txt

# ADR-0219: what a commit drops reaches every session that reads it
e14-cross-session:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0219 - what a commit drops reaches every session that reads it"; echo; \
       echo "produced by: just e14-cross-session"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_post_reaches_every_open_timeline a_post_over_http_reaches_another_reader_after_its_answer a_page_reading_nothing_dropped_is_told_nothing a_sessions_own_change_reaches_no_other_session the_feed_is_served_by_the_host_its_data_the_deployments 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/cross_session_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/cross_session_mutations.py; \
     } > docs/evidence/E14/cross-session.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/cross-session.txt

# ADR-0217: what a handler at the top of the page captures is set again when it changes
e14-top-captures:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0217 - what a handler at the top of the page captures is set again when it changes"; echo; \
       echo "produced by: just e14-top-captures"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the plan and the speculation (compiler/pw-core/tests/top_level_captures.rs)"; echo; \
       cargo test --locked -p pw-core --test top_level_captures 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server's patch, and the browser's renderer"; echo; \
       cargo test --locked -p pw-dev-server -- a_handlers_captures_at_the_top_of_the_page_are_set_again 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-render-wasm 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/top_captures_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/top_captures_mutations.py; \
     } > docs/evidence/E14/top-captures.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/top-captures.txt

# ADR-0216: every clause belongs to a declaration that reads it, and a code body admits none
e14-clause-heads:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0216 - every clause belongs to a declaration that reads it, and a code body admits none"; echo; \
       echo "produced by: just e14-clause-heads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/clause_places.rs)"; echo; \
       cargo test --locked -p pw-core --test clause_places 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store, kiokun and the accepted corpus"; echo; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | tail -1; \
       python3 scripts/accepted_corpus.py; \
       echo; echo "== mutation controls (scripts/clause_heads_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/clause_heads_mutations.py; \
     } > docs/evidence/E14/clause-heads.txt
    @grep -E "^test result|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/clause-heads.txt

# ADR-0215: a query's retry reaches its runtime as declared, and fixed is no strategy
e14-query-retry:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0215 - a query's retry reaches its runtime as declared, and fixed is no strategy"; echo; \
       echo "produced by: just e14-query-retry"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the plan, the cache and the server"; echo; \
       cargo test --locked -p pw-core --test query_retry --test policy_values --test handlers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-resource --test gate 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-dev-server -- a_querys_retry_reaches_its_cache_as_declared 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/query_retry_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/query_retry_mutations.py; \
     } > docs/evidence/E14/query-retry.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/query-retry.txt

# ADR-0212: a query reads a query or a resource, and a subscription a subscription
e14-query-reads:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0212 - a query reads a query or a resource, and a subscription a subscription"; echo; \
       echo "produced by: just e14-query-reads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/reads_name_their_kind.rs)"; echo; \
       cargo test --locked -p pw-core --test reads_name_their_kind 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus"; echo; \
       cargo test --locked -p pw-core --test generality 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/query_reads_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/query_reads_mutations.py; \
     } > docs/evidence/E14/query-reads.txt
    @grep -E "^test result|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/query-reads.txt

# ADR-0213: List.maximum gives +0 over -0
e14-float-maximum:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0213 - List.maximum gives +0 over -0"; echo; \
       echo "produced by: just e14-float-maximum"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== both backends, by bits (compiler/pw-conformance/tests/stdlib.rs, javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test stdlib -- sum_and_maximum 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-conformance --test javascript 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/float_maximum_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/float_maximum_mutations.py; \
     } > docs/evidence/E14/float-maximum.txt
    @grep -E "^test result|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/float-maximum.txt

# ADR-0214: an opaque type's own module reads its representation as .value, and declares no member of that name
e14-opaque-value:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0214 - an opaque type's own module reads its representation as .value, and declares no member of that name"; echo; \
       echo "produced by: just e14-opaque-value"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/members.rs)"; echo; \
       cargo test --locked -p pw-core --test members 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the accepted corpus, A-015 to A-017 included"; echo; \
       python3 scripts/accepted_corpus.py; \
       echo; echo "== mutation controls (scripts/opaque_value_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/opaque_value_mutations.py; \
     } > docs/evidence/E14/opaque-value.txt
    @grep -E "^test result|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/opaque-value.txt

# ADR-0211: a path that leaves a loop's body leaves the function, and owes
# its releases. The tests, the corpus, and the mutation controls.
e14-affine-loops:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0211 - a path that leaves a loop's body leaves the function, and owes its releases"; echo; \
       echo "produced by: just e14-affine-loops"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/affine_loops.rs)"; echo; \
       cargo test --locked -p pw-core --test affine_loops --test affine_bindings 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus"; echo; \
       cargo test --locked -p pw-core --test generality 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/affine_loops_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/affine_loops_mutations.py; \
     } > docs/evidence/E14/affine-loops.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/affine-loops.txt

# ADR-0209: a command computes the entries it invalidates. The command's
# calls, the server's drops, the store against its references, and the
# mutation controls.
e14-command-invalidations:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0209 - a command computes the entries it invalidates"; echo; \
       echo "produced by: just e14-command-invalidations"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the command's component (compiler/pw-conformance/tests/invalidations.rs)"; echo; \
       cargo test --locked -p pw-conformance --test invalidations 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's commands, against their references"; echo; \
       cargo test --locked -p pw-conformance --test oracle 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server's drops"; echo; \
       cargo test --locked -p pw-dev-server -- an_invalidated_entry_is_dropped_by_the_key_the_command_computed 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's artifacts, as the compiler emits them"; echo; \
       cargo test --locked -p pw-core --test evidence_is_current 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/command_invalidations_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/command_invalidations_mutations.py; \
     } > docs/evidence/E14/command-invalidations.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/command-invalidations.txt

# ADR-0208: a command computes its events, and the outbox commits them with
# its writes. The component's calls, the outbox's values, the server's
# commits, and the mutation controls.
e14-command-events:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0208 - a command computes its events, and the outbox commits them with its writes"; echo; \
       echo "produced by: just e14-command-events"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the command's component (compiler/pw-conformance/tests/events.rs)"; echo; \
       cargo test --locked -p pw-conformance --test events 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the outbox's values (runtime/pw-materialize/tests/outbox_values.rs)"; echo; \
       cargo test --locked -p pw-materialize --test outbox_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server's commits"; echo; \
       cargo test --locked -p pw-dev-server -- a_command_commits_the_events_it_computes a_command_that_emits_nothing_tells_nothing a_command_whose_events_cannot_be_kept_is_not_run 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's artifacts, as the compiler emits them"; echo; \
       cargo test --locked -p pw-core --test evidence_is_current 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/committed_events_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/committed_events_mutations.py; \
     } > docs/evidence/E14/command-events.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/command-events.txt

# ADR-0207: a data source states what it guarantees, and nothing asks it for
# more. The rules' tests, the store checked with its source, and the
# mutation controls.
e14-data-sources:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0207 - a data source states what it guarantees, and nothing asks it for more"; echo; \
       echo "produced by: just e14-data-sources"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rules, and the store's source (compiler/pw-core/tests/data_sources.rs)"; echo; \
       cargo test --locked -p pw-core --test data_sources 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store, checked with examples/lib/StoreData.pw"; echo; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw 2>&1 | tail -3; \
       echo; echo "== mutation controls (scripts/data_sources_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/data_sources_mutations.py; \
     } > docs/evidence/E14/data-sources.txt
    @grep -E "^test result|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/data-sources.txt

# ADR-0204: an element holds only the children HTML permits, as the page
# holds them. The rule's tests, R-019's, and the mutation controls.
e14-rendered-children:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0204 - an element holds only the children HTML permits, as the page holds them"; echo; \
       echo "produced by: just e14-rendered-children"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/children_as_rendered.rs)"; echo; \
       cargo test --locked -p pw-core --test children_as_rendered 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus's expectations, R-019's among them (compiler/pw-core/tests/checking_source.rs)"; echo; \
       cargo test --locked -p pw-core --test checking_source 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/rendered_children_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/rendered_children_mutations.py; \
     } > docs/evidence/E14/rendered-children.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/rendered-children.txt

# ADR-0201 (ADR-0195's ruling 5): a case written alone is the case of the
# type expected where it is written. Its tests, the components and the
# JavaScript modules, and the mutation controls.
e14-expected-cases:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0201 - a case written alone is the case of the type expected where it is written"; echo; \
       echo "produced by: just e14-expected-cases"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/sum_types.rs)"; echo; \
       cargo test --locked -p pw-core --test sum_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== built (compiler/pw-conformance/tests/sum_types.rs, javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test sum_types a_case_written_alone 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-conformance --test javascript a_case_from_its_expected -- --nocapture 2>&1 | grep -E '^javascript:|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/expected_cases_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/expected_cases_mutations.py; \
     } > docs/evidence/E14/expected-cases.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E14/expected-cases.txt

# ADR-0200: `()` is the unit value, and no expression the compiler cannot
# read checks. Its tests, the repository's every `.pw` file, and the mutation
# controls.
e14-unit-value:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0200 - () is the unit value, and no expression the compiler cannot read checks"; echo; \
       echo "produced by: just e14-unit-value"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/unit_value.rs)"; echo; \
       cargo test --locked -p pw-core --test unit_value 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== every .pw file in the repository (compiler/pw-core/tests/every_expression_is_read.rs)"; echo; \
       cargo test --locked -p pw-core --test every_expression_is_read 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a block left open (pw-syntax's grammar, compiler/pw-core/tests/template_blocks.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib a_block_left_open 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --test template_blocks an_unclosed_block 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/unit_value_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/unit_value_mutations.py; \
     } > docs/evidence/E14/unit-value.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/unit-value.txt

# ADR-0199 (ADR-0195's ruling 12): a function or a command named as a
# handler is the lambda that calls it. Its tests, the store's handlers, and
# the mutation controls.
e14-named-handlers:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0199 - a function or a command named as a handler is the lambda that calls it"; echo; \
       echo "produced by: just e14-named-handlers"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/named_handlers.rs)"; echo; \
       cargo test --locked -p pw-core --test named_handlers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what it performs, and a local (handlers_in_the_browser.rs, every_handler_is_resumable.rs)"; echo; \
       cargo test --locked -p pw-core --test handlers_in_the_browser 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --test every_handler_is_resumable 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/named_handlers_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/named_handlers_mutations.py; \
     } > docs/evidence/E14/named-handlers.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/named-handlers.txt

# ADR-0198 (ADR-0195's ruling 5): a bare case with a payload is the case of
# the one type that has it. Its tests, the components and the JavaScript
# modules, and the mutation controls.
e14-bare-cases:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0198 - a bare case with a payload is the case of the one type that has it"; echo; \
       echo "produced by: just e14-bare-cases"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/sum_types.rs)"; echo; \
       cargo test --locked -p pw-core --test sum_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== built (compiler/pw-conformance/tests/sum_types.rs, javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test sum_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-conformance --test javascript every_query_agrees -- --nocapture 2>&1 | grep -E '^javascript:|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/bare_cases_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/bare_cases_mutations.py; \
     } > docs/evidence/E14/bare-cases.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E14/bare-cases.txt

# ADR-0197 (ADR-0195's ruling 2): a pattern tells a case from a binding by
# its capital. Its tests, the conformance suite's patterns, the corpus at C14,
# and the mutation controls.
e14-case-names:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0197 - a pattern tells a case from a binding by its capital"; echo; \
       echo "produced by: just e14-case-names"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/case_names.rs, sum_types.rs)"; echo; \
       cargo test --locked -p pw-core --test case_names --test sum_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== compiled patterns (compiler/pw-conformance/tests/patterns.rs, sum_types.rs)"; echo; \
       cargo test --locked -p pw-conformance --test patterns --test sum_types 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== the corpus at C14 (corpus-check, generality.rs, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test generality --test checking_source 2>&1 \
         | grep -E '^test (generality_is|every_rejected|every_caught|a_caught_file|every_diagnostic)|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/case_names_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/case_names_mutations.py; \
     } > docs/evidence/E14/case-names.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/case-names.txt

# ADR-0196 (ADR-0195's ruling 3): only a word that begins a statement or an
# expression is reserved. The parser's test, every program checked as before,
# and the mutation controls.
e14-reserved-words:
    @cargo build --quiet --locked -p pw-cli
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0196 - only a word that begins a statement or an expression is reserved"; echo; \
       echo "produced by: just e14-reserved-words"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the parser (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax -- a_statement_keyword_cannot_name_a_value 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== every program checks as before"; echo; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -3; \
       echo; echo "== mutation controls (scripts/reserved_words_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/reserved_words_mutations.py; \
     } > docs/evidence/E14/reserved-words.txt
    @grep -E "^test result|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/reserved-words.txt

# ADR-0194: a type that contains itself compiles, and crosses a boundary as
# its nodes. The checker's, the WIT generator's and the lowering's tests; the
# components through the E8 host against a Rust model; the JavaScript modules
# against the components under Node; and the mutation controls.
e14-recursive-types:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0194 - a type that contains itself compiles, and crosses as its nodes"; echo; \
       echo "produced by: just e14-recursive-types"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the checker, the WIT and the lowering (compiler/pw-core/tests/recursive_types.rs)"; echo; \
       cargo test --locked -p pw-core --test recursive_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== components through the E8 host (compiler/pw-conformance/tests/recursive_types.rs)"; echo; \
       cargo test --locked -p pw-conformance --test recursive_types -- --test-threads=1 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the JavaScript modules, against their components under Node (compiler/pw-conformance/tests/javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test javascript a_type_that_contains_itself -- --nocapture 2>&1 | grep -E '^javascript:|^test result|^---- |panicked at'; \
       echo; echo "== the corpus at C13 (corpus-check, generality.rs, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test generality --test checking_source 2>&1 \
         | grep -E '^test (generality_is|every_rejected|every_caught|a_caught_file|every_diagnostic)|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/recursive_types_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/recursive_types_mutations.py; \
     } > docs/evidence/E14/recursive-types.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E14/recursive-types.txt

# ADR-0186: a page states its description. The compiler's, the renderer's
# and the server's tests, each store's page in three engines, the corpus at
# C11, every program clean, and the mutation controls.
e14-metadata:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0186 - a page states its description"; echo; \
       echo "produced by: just e14-metadata"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the rule, the template and the plan (compiler/pw-core/tests/metadata.rs)"; echo; \
       cargo test --locked -p pw-core --test metadata 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer and its document (runtime/pw-render/tests/metadata.rs, titles.rs)"; echo; \
       cargo test --locked -p pw-render --test metadata --test titles 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^test tests::(a_store_s_page_describes_itself|a_signal_page_s_head_holds)|^test result|^---- |panicked at'; \
       echo; echo "== each store's page, in three engines (e2e/stores.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stores.spec.mjs --reporter=list; \
       echo; echo "== the corpus at C11 (corpus-check, generality.rs, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test generality --test checking_source 2>&1 \
         | grep -E '^test (generality_is|every_rejected|every_caught|a_caught_file|every_diagnostic)|^test result|^---- |panicked at'; \
       echo; echo "== every program the repository checks"; echo; \
       python3 scripts/accepted_corpus.py; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw examples/demo/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | tail -1; \
       cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         benchmarks/baselines/pleris/domain.pw benchmarks/baselines/pleris/lib/*.pw benchmarks/baselines/pleris/store/*.pw 2>&1 | tail -1; \
       echo; echo "== mutation controls (scripts/metadata_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/metadata_mutations.py; \
     } > docs/evidence/E14/metadata.txt
    @grep -E "^test result|passed|no diagnostics|mutants killed|^---- |panicked at" docs/evidence/E14/metadata.txt

# ADR-0187: nothing the store contains is on screen when its page is first
# laid out. The page in three engines, a thousand items alone in Chromium
# (E7 gate item 10), and the mutation controls.
e14-stable-layout:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0187 - nothing the store contains is on screen when its page is first laid out"; echo; \
       echo "produced by: just e14-stable-layout"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the page, in three engines (e2e/stable-layout.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stable-layout.spec.mjs --reporter=list; \
       echo; echo "== a thousand items, alone in Chromium (e2e/performance.spec.mjs, E7 gate item 10)"; echo; \
       PW_PERFORMANCE=1 bash scripts/run_suite.sh -- pnpm exec playwright test e2e/performance.spec.mjs --project=chromium --workers=1 --reporter=list \
          -g "gate 10"; \
       echo; echo "== mutation controls (scripts/stable_layout_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/stable_layout_mutations.py; \
     } > docs/evidence/E14/stable-layout.txt
    @grep -E "passed|failed|mutants killed" docs/evidence/E14/stable-layout.txt

# ADR-0188: a page's runtime is bounded as it is sent, in every run. The
# sizes in Chromium, and the controls that grow the runtime and the renderer
# past their bounds. E7's own record is `just e7-performance`.
e14-runtime-size:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0188 - a page's runtime is bounded as it is sent, in every run"; echo; \
       echo "produced by: just e14-runtime-size"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the sizes, as served and as sent (e2e/runtime-size.spec.mjs, Chromium)"; echo; \
       PW_PERFORMANCE=1 bash scripts/run_suite.sh -- pnpm exec playwright test e2e/runtime-size.spec.mjs --project=chromium --workers=1 --reporter=list; \
       echo; echo "== controls (scripts/runtime_size_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/runtime_size_mutations.py; \
     } > docs/evidence/E14/runtime-size.txt
    @grep -E "EVIDENCE interactive|EVIDENCE renderer|passed|failed|mutants killed" docs/evidence/E14/runtime-size.txt

# ADR-0153 and ADR-0154: the two forms gate item 5 found open. A template
# tests a case with `{#match}` (PW0337), and a command a page's handler calls
# declares `idempotent_by` (PW0338). The tests and the mutation controls.
e14-case-and-delivery:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0153, ADR-0154 - a case tested with {#match}, a command a page sends declared idempotent"; echo; \
       echo "produced by: just e14-case-and-delivery"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the tests (compiler/pw-core/tests/case_and_delivery.rs)"; echo; \
       cargo test --locked -p pw-core --test case_and_delivery 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store, the accepted corpus and the demos check as before"; echo; \
       python3 scripts/accepted_corpus.py; for program in store demo; do \
         printf '%s: ' "$program"; \
         cargo run --quiet --locked -p pw-cli -- check packages/pw-std/*.pw packages/pw-platform-web/*.pw \
           examples/domain.pw examples/lib/*.pw examples/$program/*.pw 2>&1 | tail -1; \
       done; \
       echo; echo "== mutation controls (scripts/case_and_delivery_mutations.py)"; echo; \
       python3 scripts/case_and_delivery_mutations.py; \
       echo; \
       echo "Each program is checked as one: the store's pages and the corpus's both"; \
       echo "serve \`/cart\`, which one program may not (PW0341). Until 2026-10-11 the"; \
       echo "three were checked as one, and a route each serves was an error."; \
     } > docs/evidence/E14/case-and-delivery.txt
    @grep -E "^test result|mutants killed|pw check|^---- |panicked at" docs/evidence/E14/case-and-delivery.txt

# E14's gate item 5: where each task's plausible wrong fix is caught, per
# stack, from the tasks' recorded controls, and the rules `pw check` refuses
# Pleris's with. Record each task's controls first (`just e14-harness T..`).
e14-unsafe-table:
    @cargo build --quiet --locked -p pw-cli
    @mkdir -p docs/evidence/E14
    @{ echo "produced by: just e14-unsafe-table"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo; python3 scripts/unsafe_table.py; \
     } > docs/evidence/E14/unsafe-table.txt
    @tail -4 docs/evidence/E14/unsafe-table.txt

# ADR-0151: a page's values are read outside the subscriber table, so pages
# read at once share a query's flight, and a change that reaches a session
# while its page is read is not lost. Every server test, and the mutation
# controls.
e14-document-reads:
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0151 - a page's values are read outside the subscriber table"; echo; \
       echo "produced by: just e14-document-reads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the server's tests (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/document_reads_mutations.py)"; echo; \
       python3 scripts/document_reads_mutations.py; \
     } > docs/evidence/E14/document-reads.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/document-reads.txt

# ADR-0149: `pw diff` over each benchmark task's Pleris reference and unsafe
# patches, the report a reviewer of each would be shown, and the mutation
# controls. E14's gate item 1.
e14-diffs:
    @cargo build --quiet --locked -p pw-cli
    @{ echo "ADR-0149 - pw diff, what a change means, for review"; echo; \
       echo "produced by: just e14-diffs"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the tests (compiler/pw-core/tests/semantic_diff.rs)"; echo; \
       cargo test --locked -p pw-core --test semantic_diff 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== each task's Pleris patches (scripts/semantic_diffs.py)"; echo; \
       python3 scripts/semantic_diffs.py; \
       echo; echo "== mutation controls (scripts/semantic_mutations.py)"; echo; \
       python3 scripts/semantic_mutations.py; \
     } > docs/evidence/E14/semantic-diffs.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/semantic-diffs.txt

# ADR-0148: a stream region shows its query's state, and its settled arm comes
# in the same response. The rules, the IR and the plan, the renderer, the
# server's streamed response, the browser's spec in three engines and in the
# host's Chrome, and the mutation controls, against one staged build.
e14-streams:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @{ echo "ADR-0148 - a stream region shows its query's state, and its settled arm comes in the same response"; echo; \
       echo "produced by: just e14-streams"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the rules and the typer (compiler/pw-core/tests/streams.rs)"; echo; \
       cargo test --locked -p pw-core --test streams 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the IR and the plan (compiler/pw-core/tests/stream_plan.rs)"; echo; \
       cargo test --locked -p pw-core --test stream_plan 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer (runtime/pw-render/tests/streams.rs)"; echo; \
       cargo test --locked -p pw-render --test streams 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server, its streamed response among it (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the accepted corpus, A-008 among it, checks as one program"; echo; \
       python3 scripts/accepted_corpus.py; \
       echo; echo "== the browser (e2e/stream.spec.mjs), three engines and the host's Chrome"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stream.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/streams_mutations.py)"; echo; \
       python3 scripts/streams_mutations.py; \
     } > docs/evidence/E14/streams.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/streams.txt

# ADR-0148's measurements: how each engine treats an out-of-order streamed
# patch, when each way of loading a script runs while a response is open, and
# when each first paints. Chrome is the host's, where one is installed.
e14-stream-probes:
    @{ echo "ADR-0148 - out-of-order streaming, script start and first paint, per engine"; echo; \
       echo "produced by: just e14-stream-probes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       (cd spikes/own-renderer && node probes/streaming.mjs 2>&1); \
     } > docs/evidence/E14/stream-probes.txt
    @grep -E "^[a-z]+ \(" docs/evidence/E14/stream-probes.txt

# ADR-0146: a block a query decides is rendered and kept current. The plan's
# tests, the server's, the browser's spec in three engines, and the mutation
# controls, against one staged build.
e14-query-blocks:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @{ echo "ADR-0146 - a block a query decides is rendered and kept current"; echo; \
       echo "produced by: just e14-query-blocks"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the plan (compiler/pw-core/tests/page_blocks.rs)"; echo; \
       cargo test --locked -p pw-core --test page_blocks 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser (e2e/resource-path.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/resource-path.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/query_blocks_mutations.py)"; echo; \
       python3 scripts/query_blocks_mutations.py; \
     } > docs/evidence/E14/query-blocks.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/query-blocks.txt

# ADR-0145: a page keeps every part and list its queries decide current. The
# server's tests, the protocol's, `pw-render --plan`'s, the browser's specs in
# three engines, and the mutation controls, against one staged build.
e14-patch-set:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @{ echo "ADR-0145 - a page keeps every part and list its queries decide current"; echo; \
       echo "produced by: just e14-patch-set"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the server (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the protocol (runtime/pw-protocol)"; echo; \
       cargo test --locked -p pw-protocol 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== pw-render --plan (runtime/pw-render/tests/plan_lists.rs)"; echo; \
       cargo test --locked -p pw-render --test plan_lists 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser (e2e/resource-path.spec.mjs, e2e/store.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/resource-path.spec.mjs e2e/store.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/patch_set_mutations.py)"; echo; \
       python3 scripts/patch_set_mutations.py; \
     } > docs/evidence/E14/patch-set.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/patch-set.txt

# ADR-0144: a view's own signals, and a signal a page provides. The
# compiler's tests, the browser's spec in three engines, and the mutation
# controls, compiler and browser, against one staged build.
e14-provide:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @{ echo "ADR-0144 - a view's own signals, and a signal a page provides"; echo; \
       echo "produced by: just e14-provide"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/provide.rs)"; echo; \
       cargo test --locked -p pw-core --test provide 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser (spikes/own-renderer/e2e/provide.spec.mjs), three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/provide.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/provide_mutations.py)"; echo; \
       python3 scripts/provide_mutations.py; \
     } > docs/evidence/E14/provide.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/provide.txt

# ADR-0143: what names a form control. The rule's tests, its generality
# witnesses, each witness checked by `pw check`, and the mutation controls;
# the browser's half is `e2e/parsed-tree.spec.mjs`, in the own-renderer suite.
e14-labels:
    @{ echo "ADR-0143 - what names a form control"; echo; \
       echo "produced by: just e14-labels"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the rule (compiler/pw-core/tests/labels.rs)"; echo; \
       cargo test --locked -p pw-core --test labels 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== each witness (examples/generality/control_without_label)"; echo; \
       cargo build --quiet --locked -p pw-cli; \
       for f in examples/generality/control_without_label/*.pw; do \
         printf '%-24s %-10s %s\n' "$(basename $f)" "$(grep -m1 '@status:' $f | sed 's#// @status: ##')" \
           "$(./target/debug/pw check $f 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -oE '\[PW5014\][^[]*|no diagnostics' | head -1)"; \
       done; \
       echo; echo "== the generality suite (compiler/pw-core/tests/generality.rs)"; echo; \
       cargo test --locked -p pw-core --test generality 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/labels_mutations.py)"; echo; \
       python3 scripts/labels_mutations.py; \
     } > docs/evidence/E14/labels.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/labels.txt

# ADR-0061: a declared sum type in a template's `{#match}`. The checker's
# reading of each arm, the template IR's names for the cases, the renderer
# binding a case's fields, kiokun's server carrying a case, and the mutation
# controls.
e10-template-sum-types:
    @{ echo "ADR-0061 - a declared sum type in a template's match"; echo; \
       echo "produced by: just e10-template-sum-types"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker and the template IR (compiler/pw-core/tests/template_blocks.rs)"; echo; \
       cargo test --locked -p pw-core --test template_blocks 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer (runtime/pw-render/tests/branches.rs)"; echo; \
       cargo test --locked -p pw-render --test branches 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== kiokun's server"; echo; \
       cargo test --locked -p kiokun-server a_declared_case 2>&1 | grep -E '^(test |test result: ok. [1-9])|panicked at'; \
       echo; echo "== mutation controls (scripts/template_match_mutations.py)"; echo; \
       python3 scripts/template_match_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a nested or literal pattern in a template arm. NOT CLAIMED:"; \
       echo "patches for a {#match} region, which renders on the server."; \
     } > docs/evidence/E10/template-sum-types.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/template-sum-types.txt

# ADR-0060: nested and literal patterns. The checker's analysis and the
# typing of what a nested pattern binds, each match compiled to a decision
# tree and run through the E8 host against a Rust model, the component
# against the JavaScript module, and the mutation controls. Needs `node`.
e10-patterns:
    @{ echo "ADR-0060 - nested and literal patterns"; echo; \
       echo "produced by: just e10-patterns"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/patterns.rs)"; echo; \
       cargo test --locked -p pw-core --test patterns 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== through the E8 host, against a model (compiler/pw-conformance/tests/patterns.rs)"; echo; \
       cargo test --locked -p pw-conformance --test patterns 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*|^test result.*"; \
       echo; echo "== mutation controls (scripts/pattern_mutations.py)"; echo; \
       python3 scripts/pattern_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a Float literal pattern, refused by name. NOT CLAIMED: an"; \
       echo "or-pattern that binds a name, refused by name. NOT CLAIMED: an arm no"; \
       echo "value reaches reported by the checker; the backend refuses one."; \
     } > docs/evidence/E10/patterns.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E10/patterns.txt

# The 2026-09-26 correction: a Pleris name WIT reserves (`List`, `own`) is
# escaped in the generated WIT text. Each query run through the E8 host, and
# the mutation controls.
e10-wit-names:
    @{ echo "WIT's reserved names, escaped"; echo; \
       echo "produced by: just e10-wit-names"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== through the E8 host (compiler/pw-conformance/tests/wit_names.rs)"; echo; \
       cargo test --locked -p pw-conformance --test wit_names 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/wit_name_mutations.py)"; echo; \
       python3 scripts/wit_name_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a host interface or operation named by a keyword. Its name"; \
       echo "is the author's WIT, written verbatim from its host clause."; \
     } > docs/evidence/E10/wit-names.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/wit-names.txt

# ADR-0059: declared sum types. The checker's typing of a case where it is
# written, each case built and matched through the E8 host against a Rust
# model, the component against the JavaScript module, and the mutation
# controls. Needs `node`.
e10-sum-types:
    @{ echo "ADR-0059 - declared sum types"; echo; \
       echo "produced by: just e10-sum-types"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/sum_types.rs)"; echo; \
       cargo test --locked -p pw-core --test sum_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== through the E8 host, against a model (compiler/pw-conformance/tests/sum_types.rs)"; echo; \
       cargo test --locked -p pw-conformance --test sum_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*|^test result.*"; \
       echo; echo "== mutation controls (scripts/sum_type_mutations.py)"; echo; \
       python3 scripts/sum_type_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a type that contains itself in the backend, refused by"; \
       echo "name (a generic sum type is ADR-0062's, a nested or literal pattern"; \
       echo "ADR-0060's, a sum type in a template's {#match} ADR-0061's). NOT CLAIMED:"; \
       echo "a captured sum type in a handler; == between two cases."; \
     } > docs/evidence/E10/sum-types.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E10/sum-types.txt

# ADR-0057: maps and sets. Each operation against BTreeMap and BTreeSet, the
# checks on what arrives from outside, the component against the JavaScript
# module, and the mutation controls. Needs `node`.
e10-maps:
    @{ echo "ADR-0057 - maps and sets"; echo; \
       echo "produced by: just e10-maps"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== against BTreeMap and BTreeSet (compiler/pw-conformance/tests/maps.rs)"; echo; \
       cargo test --locked -p pw-conformance --test maps 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*|test the_modules_entry_check.*|^test result.*"; \
       echo; echo "== mutation controls (scripts/map_mutations.py)"; echo; \
       python3 scripts/map_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a key of a type other than Int or String, which is refused"; \
       echo "by name. NOT CLAIMED: a map or set inside a parameter's value or a"; \
       echo "host's answer, which is refused because nothing would check it; or a"; \
       echo "for loop over a map or set, which reads it through keys or to_list."; \
     } > docs/evidence/E10/maps.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E10/maps.txt

# ADR-0056: Unicode case mapping. The tables against Rust for every code
# point, the component and the module against Rust and each other, and the
# mutation controls. Needs `node`.
e10-case:
    @{ echo "ADR-0056 - Unicode case mapping"; echo; \
       echo "produced by: just e10-case"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the tables (compiler/pw-core/tests/case_mapping.rs)"; echo; \
       cargo test --locked -p pw-core --test case_mapping 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component through the host (pw-conformance/tests/case_mapping.rs)"; echo; \
       cargo test --locked -p pw-conformance --test case_mapping 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the module under Node, and the component against it"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*|test the_modules_case_mapping.*|^test result.*"; \
       echo; echo "== mutation controls (scripts/case_mutations.py)"; echo; \
       python3 scripts/case_mutations.py; \
       echo; \
       echo "NOT CLAIMED: Unicode's Final_Sigma rule. A word-final capital sigma"; \
       echo "lowers to sigma, not final sigma. NOT CLAIMED: case folding, or a"; \
       echo "locale's rules."; \
     } > docs/evidence/E10/case.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E10/case.txt

# ADR-0055: slicing, and the standard library's placeholders computed. The
# operations against Vec and str, the component against the JavaScript
# module, and the mutation controls.
e10-slices:
    @{ echo "ADR-0055 - slicing, and the standard library's placeholders computed"; echo; \
       echo "produced by: just e10-slices"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== against Vec and str (compiler/pw-conformance/tests/stdlib.rs)"; echo; \
       cargo test --locked -p pw-conformance --test stdlib 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module, on the same arguments"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*"; \
       echo; echo "== mutation controls (scripts/slice_mutations.py)"; echo; \
       python3 scripts/slice_mutations.py; \
       echo; \
       echo "NOT CLAIMED: String.split, List.range, or an Int sum. Maps and sets"; \
       echo "are not claimed here, nor Unicode case mapping (ADR-0056)."; \
     } > docs/evidence/E10/slices.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E10/slices.txt

# ADR-0054: an opaque value is built and read inside a component. The opaque
# values through the host, the component against the JavaScript module, the
# member rule over a generic representation, and the mutation controls.
e10-opaque:
    @{ echo "ADR-0054 - an opaque value is built and read inside a component"; echo; \
       echo "produced by: just e10-opaque"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== opaque values through the host (pw-conformance/tests/opaque.rs)"; echo; \
       cargo test --locked -p pw-conformance --test opaque 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module, on the same arguments"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*"; \
       echo; echo "== a generic representation's .value (pw-core/tests/members.rs)"; echo; \
       cargo test --locked -p pw-core --test members 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/opaque_mutations.py)"; echo; \
       python3 scripts/opaque_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a generic opaque type inside a component, which is refused"; \
       echo "as a generic record is. NOT CLAIMED: an opaque type's invariant; none is"; \
       echo "stated."; \
     } > docs/evidence/E10/opaque.txt
    @grep -E "^test result|^javascript:|mutants killed|^---- |panicked at" docs/evidence/E10/opaque.txt

# ADR-0053: a lambda's parameters take the types its use declares. The
# callback tests, the effect chain through a callback, what must stay clean,
# and the mutation controls.
e10-callbacks:
    @cargo build --quiet --locked -p pw-cli
    @{ echo "ADR-0053 - a lambda's parameters take the types its use declares"; echo; \
       echo "produced by: just e10-callbacks"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the callback tests (compiler/pw-core/tests/callback_parameters.rs)"; echo; \
       cargo test --locked -p pw-core --test callback_parameters 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== an effect through a callback (compiler/pw-core/tests/causal_evidence.rs)"; echo; \
       cargo test --locked -p pw-core --test causal_evidence 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what must stay clean: errors reported"; echo; \
       python3 scripts/accepted_corpus.py; \
       printf 'store: %s\n' "$(./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -cE '^error' || true)"; \
       printf 'kiokun: %s\n' "$(./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -cE '^error' || true)"; \
       echo; echo "== mutation controls (scripts/callback_mutations.py)"; echo; \
       python3 scripts/callback_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a parameter whose name is bound at two sites in one body."; \
       echo "The value relations' environment is flat, and such a name is unknown."; \
       echo "NOT CLAIMED: a lambda in a branch of an annotated binding's initialiser,"; \
       echo "typed by the annotation. A returned value is seen through its branches;"; \
       echo "an initialiser is typed only when it is the lambda itself."; \
     } > docs/evidence/E10/callbacks.txt
    @grep -E "^test result|^(accepted corpus|store|kiokun):|mutants killed|^---- |panicked at" docs/evidence/E10/callbacks.txt

# ADR-0051: an early `return`, `?`, and `for` loops compile, and the checker
# says what may be assigned. The compiled bodies through the host, the
# component against the JavaScript module, and the mutation controls.
e10-control-flow:
    @{ echo "ADR-0051 - an early return, ?, and for loops compile"; echo; \
       echo "produced by: just e10-control-flow"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== compiled bodies through the host (pw-conformance/tests/control_flow.rs)"; echo; \
       cargo test --locked -p pw-conformance --test control_flow 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what may be assigned (compiler/pw-core/tests/assignments.rs)"; echo; \
       cargo test --locked -p pw-core --test assignments 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module, on the same arguments"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*"; \
       echo; echo "== mutation controls (scripts/control_flow_mutations.py)"; echo; \
       python3 scripts/control_flow_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a return or assignment inside a lambda a list operation"; \
       echo "runs, or an assignment to a field. A function value is ADR-0052's:"; \
       echo "just e10-function-values."; \
     } > docs/evidence/E10/control-flow.txt
    @grep -E "^javascript:|mutants killed" docs/evidence/E10/control-flow.txt

# ADR-0050: recursion and generic callees compile. The compiled recursions
# through the host, the component against the JavaScript module, and the
# mutation controls.
e10-recursion:
    @{ echo "ADR-0050 - recursion and generic callees compile"; echo; \
       echo "produced by: just e10-recursion"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== recursions through the host (pw-conformance/tests/recursion.rs)"; echo; \
       cargo test --locked -p pw-conformance --test recursion 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the component and the JavaScript module, on the same arguments"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*"; \
       echo; echo "== mutation controls (scripts/recursion_mutations.py)"; echo; \
       python3 scripts/recursion_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a generic record. An early return and a loop are ADR-0051's"; \
       echo "(just e10-control-flow), a function value ADR-0052's (just e10-function-values)."; \
     } > docs/evidence/E10/recursion.txt
    @grep -E "^javascript:|mutants killed" docs/evidence/E10/recursion.txt

# ADR-0049: a string's escapes are the language's. The decoder's tests, the
# checker's, the compiled values, and the mutation controls.
e10-strings:
    @{ echo "ADR-0049 - a string's escapes are the language's"; echo; \
       echo "produced by: just e10-strings"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the decoder (compiler/pw-syntax/src/strings.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib strings 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== the checker, Koka and Marko (compiler/pw-core/tests/string_escapes.rs)"; echo; \
       cargo test --locked -p pw-core --test string_escapes 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== compiled components through the host (pw-conformance/tests/strings.rs)"; echo; \
       cargo test --locked -p pw-conformance --test strings 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== the component and the JavaScript module, on the same arguments"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: [0-9]+ queries.*"; \
       echo; echo "== mutation controls (scripts/string_mutations.py)"; echo; \
       python3 scripts/string_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a string inside a hole, or decoded policy strings."; \
     } > docs/evidence/E10/strings.txt
    @grep -E "^javascript:|mutants killed" docs/evidence/E10/strings.txt

# ADR-0048: a read names a member its value's type has (PW0610). The member
# tests, the corpus that must stay clean, and the mutation controls.
e10-members:
    @cargo build --quiet --locked -p pw-cli
    @{ echo "ADR-0048 - a read names a member its value's type has"; echo; \
       echo "produced by: just e10-members"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the member tests (compiler/pw-core/tests/members.rs)"; echo; \
       cargo test --locked -p pw-core --test members 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== what must stay clean: errors reported"; echo; \
       python3 scripts/accepted_corpus.py; \
       printf 'store: %s\n' "$(./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -cE '^error' || true)"; \
       printf 'kiokun: %s\n' "$(./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -cE '^error' || true)"; \
       echo; echo "== what the audit decided (member relations, the accepted corpus)"; echo; \
       ./target/debug/pw audit-values packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/accepted/*.pw 2>&1 | grep -E '^  Member' || true; \
       echo; echo "== mutation controls (scripts/member_mutations.py)"; echo; \
       python3 scripts/member_mutations.py; \
       echo; \
       echo "NOT CLAIMED here: an opaque value built or read inside a component. It"; \
       echo "compiles since 2026-09-25 (ADR-0054): just e10-opaque."; \
     } > docs/evidence/E10/members.txt
    @grep -E "^(accepted corpus|store|kiokun):|mutants killed" docs/evidence/E10/members.txt

# ADR-0047: every name resolves, in lexical scope (PW0021). The name tests, the
# corpus that must stay clean, and the mutation controls.
e10-names:
    @cargo build --quiet --locked -p pw-cli
    @{ echo "ADR-0047 - every name resolves, in lexical scope"; echo; \
       echo "produced by: just e10-names"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the name tests (compiler/pw-core/tests/every_name_resolves.rs)"; echo; \
       cargo test --locked -p pw-core --test every_name_resolves 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== what must stay clean: errors reported"; echo; \
       python3 scripts/accepted_corpus.py; \
       printf 'store: %s\n' "$(./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/domain.pw examples/lib/*.pw examples/store/*.pw 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -cE '^error' || true)"; \
       printf 'kiokun: %s\n' "$(./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw examples/kiokun/*.pw 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -cE '^error' || true)"; \
       echo; echo "== mutation controls (scripts/names_mutations.py)"; echo; \
       python3 scripts/names_mutations.py; \
       echo; \
       echo "NOT CLAIMED: that derived is pure, or that a clause's words are in its"; \
       echo "domain. Both are left to the analyses that read them."; \
     } > docs/evidence/E10/names.txt
    @grep -E "^(accepted corpus|store|kiokun):|mutants killed" docs/evidence/E10/names.txt

# ADR-0044: pure computation compiled to JavaScript modules, held to its Wasm
# component under Node, with the mutation controls. Needs `node`.
#
# E10 task 2 — "generated modern JavaScript modules for ... pure computation".
e10-javascript:
    @{ echo "ADR-0044 - pure computation as JavaScript modules, against the components"; echo; \
       echo "produced by: just e10-javascript"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the component and the module, on the same arguments"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "javascript: .*|^test result.*"; \
       echo; echo "== mutation controls (scripts/javascript_mutations.py)"; echo; \
       python3 scripts/javascript_mutations.py; \
       echo; \
       echo "NOT CLAIMED: a handler that computes, Wasm in the browser, or the modules"; \
       echo "in a browser engine. They run under Node."; \
     } > docs/evidence/E10/javascript.txt
    @grep -E "^javascript:|mutants killed" docs/evidence/E10/javascript.txt

# ADR-0043: an operator's operands and an `if`'s condition typed (PW0609), and
# `Float.from_int`, with the mutation controls.
#
# E10 — the checker gap KNOWN_LIMITATIONS named: `1 == "a"` checked.
e10-operands:
    @{ echo "ADR-0043 - operands are typed, and Float.from_int"; echo; \
       echo "produced by: just e10-operands"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the value relations (compiler/pw-core/tests/value_relations.rs)"; echo; \
       cargo test --locked -p pw-core --test value_relations -- --test-threads=1 2>&1 \
         | grep -E '^test (an_operator|an_untyped|the_accepted)|^test result|^---- |panicked at'; \
       echo; echo "== Float.from_int (compiler/pw-conformance/tests/stdlib.rs)"; echo; \
       cargo test --locked -p pw-conformance --test stdlib an_int_becomes -- --test-threads=1 2>&1 \
         | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/operand_mutations.py)"; echo; \
       python3 scripts/operand_mutations.py; \
       echo; \
       echo "NOT CLAIMED: an operand the checker cannot type. It is undecided, and"; \
       echo "the component backend refuses what remains."; \
     } > docs/evidence/E10/operands.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/operands.txt

# ADR-0042's template branches and attributes: the checker's refusals and the
# IR (pw-core), what renders (pw-render), and the mutation controls.
#
# E10 — `{:else}`, `{#match}` and interpolated attributes.
e10-templates:
    @{ echo "ADR-0042 - template branches and interpolated attributes"; echo; \
       echo "produced by: just e10-templates"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker and the IR (compiler/pw-core/tests/template_blocks.rs)"; echo; \
       cargo test --locked -p pw-core --test template_blocks -- --test-threads=1 2>&1 \
         | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer (runtime/pw-render/tests/branches.rs)"; echo; \
       cargo test --locked -p pw-render --test branches -- --test-threads=1 2>&1 \
         | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/template_mutations.py)"; echo; \
       python3 scripts/template_mutations.py; \
       echo; \
       echo "NOT CLAIMED: patches for a match region or an interpolated attribute."; \
       echo "They render on the server; no patch generator emits them yet."; \
     } > docs/evidence/E10/templates.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E10/templates.txt

# ADR-0041's mutation controls: each piece of kiokun's logic in Pleris, and of
# the defects found building it, undone in turn, must fail a test.
#
# E10 — the kiokun slice's shard rule and ranking, in Pleris.
e10-kiokun-mutants:
    @{ echo "ADR-0041 - kiokun's shard rule and ranking in Pleris: mutation controls"; echo; \
       echo "produced by: just e10-kiokun-mutants"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       python3 scripts/kiokun_mutations.py; \
       echo; \
       echo "NOT CLAIMED: search over every shard. Search is over the loaded shard;"; \
       echo "a lookup reaches every shard."; \
     } > docs/evidence/E10/kiokun-mutants.txt
    @grep -E "mutants killed" docs/evidence/E10/kiokun-mutants.txt

# Code size: the store's artifacts as `pw build` writes them, beside two
# baselines. One is hand-written Rust components (the E0/E8 spike guests, from
# `just spike-wasmtime`). The other is the runtime E7 recorded. Performance: the
# compiled command, a hand-written Rust guest and the native operation, through
# one host API, in release; then E7's own browser instrument, re-run into E10's
# file so E7's record is untouched.
#
# E10 gate item 4 — code size and performance, recorded against baselines.
e10-bench:
    @cargo build --quiet --locked -p pw-cli
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @EVIDENCE_FILE="$(pwd)/docs/evidence/E10/performance-e7-instrument.txt" PRODUCED_BY="just e10-bench" \
      bash spikes/own-renderer/performance.sh > /dev/null
    @{ echo "E10 gate item 4 - code size and performance against baselines"; echo; \
       echo "produced by: just e10-bench"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "host: $(uname -m) $(sysctl -n machdep.cpu.brand_string 2>/dev/null || true)"; echo; \
       out="$(mktemp -d)"; \
       ./target/debug/pw build --out "$out" packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw > /dev/null; \
       echo "== code size: the store's artifacts (bytes, gzip -9 bytes)"; \
       (cd "$out" && for f in components/*.wasm handlers/*.mjs; do \
         printf '  %6d  %6d  %s\n' "$(wc -c < "$f")" "$(gzip -9c "$f" | wc -c)" "$f"; done); \
       rm -rf "$out"; \
       echo; echo "== code size: baselines (bytes, gzip -9 bytes)"; \
       for f in spikes/wasmtime-component/guest-minimal/target/wasm32-wasip2/release/spike_wasmtime_guest_minimal.wasm \
                spikes/wasmtime-component/guest/target/wasm32-wasip2/release/spike_wasmtime_guest.wasm; do \
         if [ -f "$f" ]; then printf '  %6d  %6d  %s\n' "$(wc -c < "$f")" "$(gzip -9c "$f" | wc -c)" "$f (hand-written Rust)"; \
         else echo "  (absent: $f - run just spike-wasmtime)"; fi; done; \
       echo "  server-written handler modules before ADR-0033: add_to_cart 255, clear_cart 180 (reconstructed from 83af93c's format string)"; \
       printf '  %6d  %6d  %s\n' "$(wc -c < spikes/own-renderer/public/pw-runtime.mjs)" "$(gzip -9c spikes/own-renderer/public/pw-runtime.mjs | wc -c)" "pw-runtime.mjs now"; \
       echo "  E7's record: $(grep -oE 'interactive-script-bytes=[0-9]+' docs/evidence/E7/performance.txt) $(grep -oE 'interactive-wasm-bytes=[0-9]+' docs/evidence/E7/performance.txt)"; \
       echo; echo "== the host, release: runtime/pw-host/tests/bench.rs"; echo; \
       cargo test --quiet --locked --release -p pw-host --features engine --test bench -- --include-ignored --nocapture --test-threads=1 2>&1 \
         | grep -E "^bench:|^test result|^---- |panicked at"; \
       echo; echo "== the browser: activation, the runtime before compiled handlers and now (chromium, 1 worker, interleaved)"; echo; \
       bash spikes/own-renderer/activation-compare.sh 83af93c 7 3; \
       echo; echo "== the browser: E7's instrument, re-run (full record: performance-e7-instrument.txt)"; echo; \
       echo "  E7's record (2026-08-07, an older Chromium; activation is one sample):"; grep -E "interactive-script-bytes|interactive-wasm-bytes|activation-ms|interaction-long-frames|runtime-layout-reads" docs/evidence/E7/performance.txt | sed 's/^ */    /'; \
       echo "  now (activation is one sample; the comparison above is the measurement):"; grep -E "interactive-script-bytes|interactive-wasm-bytes|activation-ms|interaction-long-frames|runtime-layout-reads" docs/evidence/E10/performance-e7-instrument.txt | sed 's/^ */    /'; \
     } > docs/evidence/E10/bench.txt
    @cat docs/evidence/E10/bench.txt

# What the store and the accepted corpus make affine, and why: only types an
# effect row acquires. No store value is affine or annotated, and the language
# has no borrow, lifetime or move syntax. With it, charter M10 task 10's
# remaining half: carried captures serialize deterministically. Resume
# versioning is E7V's (docs/evidence/E7V/deployment-matrix.txt).
#
# E10 gate item 5 — no ownership syntax for ordinary application values.
e10-ownership:
    @{ echo "E10 gate item 5 - no ownership syntax for ordinary values; task 10 - deterministic resumable state"; echo; \
       echo "produced by: just e10-ownership"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== what is affine, and what is not (compiler/pw-core/tests/ownership.rs)"; echo; \
       cargo test --locked -p pw-core --test ownership -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "^(store|accepted corpus|not Pleris): .*|^test [a-z_]+ .*|^test result.*"; \
       echo; echo "== carried captures serialize deterministically (runtime/pw-render/tests/properties.rs)"; echo; \
       cargo test --locked -p pw-render --test properties carried 2>&1 | grep -E "^(test |test result)|panicked at"; \
       echo; echo "== resume versioning: E7V, recorded in docs/evidence/E7V/deployment-matrix.txt"; \
       grep -cE "^" docs/evidence/E7V/deployment-matrix.txt | sed 's/^/   lines: /'; \
     } > docs/evidence/E10/ownership.txt
    @cat docs/evidence/E10/ownership.txt

# E10 gate item 4 at the close, 2026-10-02: code size and the host benchmark at
# HEAD, and E7 gate 8 (no long animation frame during standard interactions)
# run RUNS times with every result kept. `just e10-bench` stops at the first
# gate-8 failure, and on this machine gate 8 fails in about half of runs at
# HEAD and at 6545029, the commit E10's bench was recorded at: a frame of
# 52-63 ms with no attributed script. Recording one passing run would be the
# lucky sample `docs/RISK_QUEUE.md` warns about, so every run is kept.
e10-close-bench RUNS="8":
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @{ echo "E10 gate item 4 at the close - size, host cost, and E7 gate 8's stability"; echo; \
       echo "produced by: just e10-close-bench {{RUNS}}"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "host: $(uname -m) $(sysctl -n machdep.cpu.brand_string 2>/dev/null || true)"; echo; \
       out="$(mktemp -d)"; \
       ./target/debug/pw build --out "$out" packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw > /dev/null; \
       echo "== code size: the store's artifacts (bytes, gzip -9 bytes)"; \
       (cd "$out" && for f in components/*.wasm handlers/*.mjs; do \
         printf '  %6d  %6d  %s\n' "$(wc -c < "$f")" "$(gzip -9c "$f" | wc -c)" "$f"; done); \
       rm -rf "$out"; \
       printf '  %6d  %6d  %s\n' "$(wc -c < spikes/own-renderer/public/pw-runtime.mjs)" "$(gzip -9c spikes/own-renderer/public/pw-runtime.mjs | wc -c)" "pw-runtime.mjs"; \
       echo "  baselines: docs/evidence/E10/bench.txt (hand-written Rust guests 5276 and 43837 bytes)"; \
       echo; echo "== the host, release: runtime/pw-host/tests/bench.rs"; echo; \
       cargo test --quiet --locked --release -p pw-host --features engine --test bench -- --include-ignored --nocapture --test-threads=1 2>&1 \
         | grep -E "^bench:|^test result|^---- |panicked at"; \
       echo; echo "== E7 gate 8, {{RUNS}} runs (chromium, 1 worker): every result"; echo; \
       echo "  A run reporting a long frame fails gate 8's assertion; it is recorded, not retried, and"; \
       echo "  once every run has run, it fails the recipe (scripts/run_suite.sh)."; echo; \
       PW_PERFORMANCE=1 PORT=3141 bash scripts/run_suite.sh --runs {{RUNS}} -- pnpm exec playwright test e2e/performance.spec.mjs \
          --project=chromium --workers=1 --reporter=list -g 'gate 8'; \
     } > docs/evidence/E10/close-bench.txt
    @cat docs/evidence/E10/close-bench.txt

# E14-A (ADR-0120): the benchmark's store in three stacks, one behavioural
# contract. Each store builds and typechecks, the contract suite passes on all
# three three times over, and each mutant store fails it. The Pleris one is
# the benchmark's frozen copy (ADR-0156), as the other two are. svelte-check
# is asked for its machine output, whose summary this reads: it writes that
# by default only where `CLAUDECODE=1` (svelte-check 4.7.6), so until
# 2026-10-11 the recipe passed in an agent's session and failed every
# verification runner.
e14-contract:
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @bash spikes/own-renderer/baseline-store.sh > /dev/null
    @mkdir -p docs/evidence/E14
    @{ echo "E14-A - one store contract, three stacks"; echo; \
       echo "produced by: just e14-contract"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "node: $(node --version)  pnpm: $(pnpm --version)"; \
       for p in next react svelte @sveltejs/kit vite typescript @playwright/test; do \
         for d in benchmarks/baselines/next-react benchmarks/baselines/sveltekit benchmarks/harness; do \
           f="$d/node_modules/$p/package.json"; [ -f "$f" ] && printf '  %-18s %s  (%s)\n' "$p" "$(node -p "require('./$f').version")" "$d"; \
         done; done | sort -u; echo; \
       echo "== builds and typechecks"; \
       (cd benchmarks/baselines/next-react && pnpm --silent build > /dev/null && echo "  next-react: built" && pnpm --silent typecheck && echo "  next-react: tsc clean"); \
       (cd benchmarks/baselines/sveltekit && pnpm --silent build > /dev/null 2>&1 && echo "  sveltekit: built" && pnpm --silent typecheck --output machine 2>&1 | grep -oE "[0-9]+ ERRORS [0-9]+ WARNINGS" | sed 's/^/  sveltekit: svelte-check /'); \
       ./target/debug/pw check packages/pw-std/*.pw packages/pw-platform-web/*.pw benchmarks/baselines/pleris/domain.pw benchmarks/baselines/pleris/lib/*.pw benchmarks/baselines/pleris/store/*.pw | sed 's/^/  pleris: /'; \
       echo; echo "== the contract, every stack, three times (benchmarks/harness/contract)"; echo; \
       bash scripts/run_suite.sh -C benchmarks/harness -- pnpm exec playwright test --repeat-each=3 --reporter=list; \
       echo; echo "== negative controls (scripts/bench_contract_mutations.py)"; echo; \
       python3 scripts/bench_contract_mutations.py; \
     } > docs/evidence/E14/contract.txt
    @cat docs/evidence/E14/contract.txt

# ADR-0121: an idempotent command runs once per interaction. The contract,
# the reservation and its bound, the server's refusals, the browser's retried
# request in three engines, and the mutation controls.
e14-idempotent-commands:
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server -p kiokun-server
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0121 - an idempotent command runs once per interaction"; echo; \
       echo "produced by: just e14-idempotent-commands"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the contract carries it (compiler/pw-core/tests/component_contract.rs)"; echo; \
       cargo test --locked -p pw-core --test component_contract idempotent 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the reservation keeps what must not run twice (runtime/pw-resource)"; echo; \
       cargo test --locked -p pw-resource --test concurrency_regressions forgetting 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server -- interaction 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser, each engine alone (e2e/idempotent-command.spec.mjs)"; echo; \
       for e in chromium firefox webkit; do \
         bash scripts/run_suite.sh -- pnpm exec playwright test e2e/idempotent-command.spec.mjs --project=$e --reporter=list; \
       done; \
       echo; echo "== mutation controls (scripts/idempotent_commands_mutations.py)"; echo; \
       python3 scripts/idempotent_commands_mutations.py; \
     } > docs/evidence/E14/idempotent-commands.txt
    @grep -E "^ +[0-9]+ (passed|failed)|^test result|mutants killed|^---- |panicked at" docs/evidence/E14/idempotent-commands.txt

# ADR-0122: an optimistic transition runs in the browser. The compiled
# speculation module under Node, the server's values and versions, the
# browser in each engine alone, and the mutation controls.
e14-optimistic:
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server -p kiokun-server
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0122 - an optimistic transition runs in the browser"; echo; \
       echo "produced by: just e14-optimistic"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)  node: $(node --version)"; echo; \
       echo "== the store's speculation module (compiler/pw-conformance/tests/speculation.rs)"; echo; \
       cargo test --locked -p pw-conformance --test speculation 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what the server sends a page that speculates (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server -- speculat 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser, each engine alone (e2e/optimistic.spec.mjs)"; echo; \
       for e in chromium firefox webkit; do \
         bash scripts/run_suite.sh -- pnpm exec playwright test e2e/optimistic.spec.mjs --project=$e --reporter=list; \
       done; \
       echo; echo "== mutation controls (scripts/optimistic_transitions_mutations.py)"; echo; \
       python3 scripts/optimistic_transitions_mutations.py; \
     } > docs/evidence/E14/optimistic.txt
    @grep -E "^ +[0-9]+ (passed|failed)|^test result|mutants killed|^---- |panicked at" docs/evidence/E14/optimistic.txt

# ADR-0123/0124: the offline benchmark harness. Its isolation self-test, and
# each task's four controls on every stack, every run's result kept. A
# control that fails fails the recipe (track evidence-integrity-2: until
# 2026-10-15 `|| true` kept it green, and T12's evidence of 2026-10-02 said
# "unsafe FAIL" in a passing recipe).
e14-harness TASK="T08":
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "E14-B - the offline harness, and {{TASK}}'s controls"; echo; \
       echo "produced by: just e14-harness {{TASK}}"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "node: $(node --version)  pnpm: $(pnpm --version)  rust: $(rustc --version)"; echo; \
       echo "== isolation (benchmarks/harness/test/isolation.test.mjs)"; echo; \
       (cd benchmarks/harness && node --test test/isolation.test.mjs 2>&1 | grep -E "^(ok|not ok) "); \
       echo; echo "== the controls (node bench.mjs controls --task {{TASK}})"; echo; \
       (cd benchmarks/harness && node bench.mjs controls --task {{TASK}} 2>&1); \
       echo; echo "== each run, from benchmarks/results/ (this recipe's runs)"; echo; \
       ls -t benchmarks/results/{{TASK}}-*.json | head -9 | sort | while read -r f; do \
         node -e 'const r=require("./"+process.argv[1]); const t=r.stages.filter(s=>"passed" in s).map(s=>`${s.name} ${s.passed}/${s.passed+s.failed}`).join(", "); console.log(`  ${r.stack} ${r.agent}: score ${r.score}${r.failed_at?" (failed at "+r.failed_at+")":""}; ${t||"no tests run"}; ${r.changed_lines} line(s)`)' "$f"; \
       done; \
     } > docs/evidence/E14/harness-{{TASK}}.txt
    @cat docs/evidence/E14/harness-{{TASK}}.txt

# Every task's four controls on the Pleris stack alone: what a change to the
# compiler, the runtime or the benchmark's store can move, without rebuilding
# the frameworks' stores, whose controls `just e14-harness TASK` records. The
# first task whose controls fail fails the recipe (track
# evidence-integrity-2).
e14-pleris-controls:
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "E14 - every task's four controls, the Pleris stack"; echo; \
       echo "produced by: just e14-pleris-controls"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "node: $(node --version)  pnpm: $(pnpm --version)  rust: $(rustc --version)"; echo; \
       for t in $(ls benchmarks/tasks | grep -oE '^T[0-9]+' | sort -u); do \
         (cd benchmarks/harness && node bench.mjs controls --task $t --stack pleris 2>&1); \
       done; \
     } > docs/evidence/E14/pleris-controls.txt
    @cat docs/evidence/E14/pleris-controls.txt

# The development server under sustained commands and session churn: frames
# held, subscribers, outbox rows and materialized entries, each bounded. The
# compiled command called 20,000 times through the E8 host, with resident memory
# measured before and after. The numbers before the bound existed are in
# docs/evidence/E10/load-2026-09-25.md.
#
# E10 gate item 3 — memory leaks and unbounded resource growth under sustained load.
e10-load:
    @{ echo "E10 gate item 3 - sustained load"; echo; \
       echo "produced by: just e10-load"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the development server (spikes/own-renderer/server)"; echo; \
       cargo test --locked -p pw-dev-server -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "^(steady|churn|idle): .*|\{\"cursor\".*|^test tests::(sustained|a_subscriber|a_forgotten)[a-z_]* .*|^test result.*"; \
       echo; echo "== the compiled command through the E8 host (runtime/pw-host/tests/pleris_component.rs)"; echo; \
       cargo test --locked -p pw-host --features engine --test pleris_component sustained -- --nocapture 2>&1 \
         | grep -oE "^sustained: .*|^test result.*"; \
       echo; \
       echo "NOT CLAIMED: per-session DATA is bounded. A cart is the store's data and"; \
       echo "is kept; what is bounded is what the server holds FOR a subscriber (its"; \
       echo "frames, its queue, its cached fragment) and what it keeps AFTER use (the"; \
       echo "outbox)."; \
     } > docs/evidence/E10/load.txt
    @cat docs/evidence/E10/load.txt

# Writes the store's modules as `pw emit-handlers` emits them to
# `docs/evidence/E10/handlers/`, held there by `evidence_is_current`, and
# records the refusal matrix, the renderer carrying exactly the paths a handler
# reads, the host typing a browser's arguments by the component's own
# parameters, and the development server's one command path.
#
# E10 — resumable handler bodies, compiled to JavaScript modules (ADR-0033).
e10-handlers:
    @rm -rf docs/evidence/E10/handlers
    @{ echo "E10 - resumable handlers, compiled to JavaScript modules"; echo; \
       echo "produced by: just e10-handlers"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the store's handlers, as pw emit-handlers writes them"; echo; \
       cargo run --quiet --locked -p pw-cli -- emit-handlers --out docs/evidence/E10/handlers \
         packages/pw-std/*.pw packages/pw-platform-web/*.pw \
         examples/domain.pw examples/lib/*.pw examples/store/*.pw; \
       for f in docs/evidence/E10/handlers/*.mjs; do echo; echo "-- $f"; cat "$f"; done; \
       echo; echo "== the compiler: modules, event parts, contracts, refusals (compiler/pw-core/tests/handlers.rs)"; echo; \
       cargo test --locked -p pw-core --test handlers -- --nocapture --test-threads=1 2>&1 \
         | grep -oE "refused \`.*|^test [a-z_]+ .*|^test result.*"; \
       echo; echo "== the committed modules are the compiler's current output"; echo; \
       cargo test --locked -p pw-core --test evidence_is_current 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the renderer carries exactly the paths a handler reads (runtime/pw-render/tests/properties.rs)"; echo; \
       cargo test --locked -p pw-render --test properties 2>&1 | grep -E "^test (an_element|a_captured|a_capture|an_event)|^test result|^---- |panicked at"; \
       echo; echo "== the host types a browser's arguments by the artifact's own parameters (runtime/pw-host/tests/pleris_component.rs)"; echo; \
       cargo test --locked -p pw-host --features engine --test pleris_component -- --nocapture --test-threads=1 2>&1 \
         | grep -oE '^\[.*\]: .*|^test (json|arguments)[a-z_]+ .*|^test result.*'; \
       echo; echo "== the development server: one command path, compiled handlers only (pw-dev-server)"; echo; \
       cargo test --locked -p pw-dev-server 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; \
       echo "NOT CLAIMED: captures are a patched part. An element carries what its"; \
       echo "handler reads as rendered; the store reads only item.id, the loop key."; \
       echo "An opaque type's invariant is checked at the boundary since ADR-0179:"; \
       echo "the host holds PositiveInt's value >= 1 as it decodes the arguments."; \
       echo "NOT CLAIMED: the event as a handler parameter. No syntax binds one, the"; \
       echo "runtime listens for a click only, and a handler with parameters is refused."; \
       echo "Computation, branches, loops and several commands compile since"; \
       echo "2026-09-25 (ADR-0058): just e10-handlers-compute."; \
     } > docs/evidence/E10/handlers.txt
    @grep -E "^test result|written to|refused \`|^---- |panicked at" docs/evidence/E10/handlers.txt | head -20

# E10-I in the browser: the store page whose Add and Clear buttons reach the
# COMPILED commands, in all three engine families, three full runs — a flake
# shows up as a run that differs, and fails the recipe once the three have
# run (scripts/run_suite.sh). Needs the Playwright browsers
# (`pnpm --filter pw-own-renderer-spike exec playwright install chromium firefox
# webkit`), so it is not part of `just ci`.
#
# `out` names the evidence file: E10-I recorded `browser-suite.txt`, the
# compiled handlers (ADR-0033) `handlers-browser-suite.txt`.
e10-browser engines="chromium firefox webkit" out="docs/evidence/E10/browser-suite.txt":
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/keyed-store.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @{ echo "E10 - the store page, compiled commands and handlers, in browser engines: {{engines}}"; echo; \
       echo "produced by: just e10-browser \"{{engines}}\" {{out}}"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       bash scripts/run_suite.sh --runs 3 -- pnpm exec playwright test --reporter=line \
         $(for e in {{engines}}; do printf -- '--project=%s ' "$e"; done); \
     } > {{out}}
    @cat {{out}}

# E8. The artifact audit, against real Wasm components.
#
# Builds the guests (wasm32-wasip2) and the wasmtime engine, so it is not part
# of `just ci` — the DECISIONS it exercises are, in
# `runtime/pw-host/tests/admission.rs`, and those need no engine. Its tests'
# output is printed when they fail (scripts/tests/test_evidence_gates.py).
e8-host:
    @bash spikes/wasmtime-component/audit.sh

# E7 gate items 7-10: runtime size, activation CPU, forced synchronous layout,
# and the large-menu case.
#
# Run ALONE, on Chromium only. Three engine families in parallel saturate the
# machine, and a long-animation-frame measurement taken under that load
# measures the load — it failed exactly that way before being separated out.
e7-performance:
    @bash spikes/own-renderer/performance.sh

# E6F. There is only one parser that decides what a `.pw` program means.
# Enumerates the real keyword tables rather than a copy of them — a test with
# its own list would be the fifth copy of the thing this milestone removed.
one-parser:
    @cargo test --quiet -p pw-core --test one_parser 2>&1 | tail -3
    @cargo test --quiet -p pw-core --test corpus_declarations 2>&1 | tail -3
    @echo "  every declaration keyword reaches a DeclKind"
    @echo "  every policy keyword reaches the HIR as a Policy, from both"
    @echo "    positions the language writes one in"
    @echo "  no crate outside pw-syntax builds a second semantic tree"

# E6. The resource dependency graph, and the materializer that consumes it.
# Regenerates the committed graph fixture, because the runtime's tests read
# real compiler output and a checked-in artifact goes stale silently.
materialize:
    @cargo run --quiet -p pw-cli -- emit-graph examples/domain.pw examples/lib/*.pw examples/store/*.pw \
        > runtime/pw-materialize/tests/store-graph.json
    @cargo test --quiet -p pw-materialize 2>&1 | grep -E 'test result|^---- |panicked at' | tail -3
    @cargo run --quiet -p pw-cli -- emit-graph --plain examples/domain.pw examples/lib/*.pw examples/store/*.pw \
        | tail -n +2
    @echo
    @echo "  gate 1  MenuChanged(47) invalidates 1 of 1000; 999 untouched"
    @echo "  gate 2  duplicate events coalesce to one regeneration"
    @echo "  gate 3  last-known-good only where the policy declares it"
    @echo "  gate 4  compiler PW5101; runtime keeps two sessions apart"
    @echo "  gate 5  8 concurrent readers, 1 regeneration (and >1 without it)"
    @echo "  gate 6  the graph above, and the JSON the runtime deserializes"

# The THIRD gate: for every syntactically representable program the compiler
# must produce output, ordinary diagnostics, or a marked internal error — never
# terminate without a report. A panic takes every other rule down with it, so a
# compiler that reports nothing looks like one that found nothing.
robustness:
    @cargo test --quiet -p pw-core --test robustness 2>&1 | tail -3
    @echo "  generator families: corpus, corpus-mutation, byte-soup,"
    @echo "                      constructor-arity, or-pattern, resolution,"
    @echo "                      label-dataflow, parser-to-HIR contract"
    @echo "  '0 panics' means 0 under THESE. Coverage-guided fuzzing is a"
    @echo "  separate figure — see \`just fuzz\` — and is never merged into"
    @echo "  this one. The arity panic proves broad generation is not a"
    @echo "  substitute for a generator aimed at a known-fragile seam."

# The SECOND score. Corpus conformance says today's specification passes;
# generality says the same guarantees survive programs the fixtures did not
# anticipate. `slips-through.pw` files are known gaps, executable so they
# cannot drift out of date.
generality:
    @cargo test --quiet -p pw-core --test generality -- --nocapture 2>&1 | grep -E '^  (corpus|generally|narrowly|generality)'

# The per-fixture enforcement table. Each rejected fixture is checked as its
# own program — the shared library, the accepted modules it imports, and the
# fixture — because five of them reuse module names with each other (E2B).
# The same test, run by `just ci` without `PW_WRITE_EVIDENCE`, holds the
# committed table to what the corpus checks now (track evidence-integrity-2:
# it was two months stale).
evidence-corpus:
    @PW_WRITE_EVIDENCE=1 cargo test --quiet -p pw-core --test checking_source \
        the_committed_enforcement_table -- --nocapture 2>&1 | grep -v '^$'
    @echo
    @grep -m1 "enforce their declared invariant" docs/evidence/E2D/corpus-enforcement.txt

# Superseded by `just evidence-corpus`. Checking all 44 fixtures as ONE program
# asks a question nobody meant — five of them reuse module names with each other
# — so the duplicate-declaration errors drown the real ones (E2B).
rejections:
    @echo "use `just evidence-corpus` — the rejected fixtures are not one program"; exit 1

# `pw explain` over an example — the semantic facts a developer would otherwise
# have to infer by reading the whole file.
explain FILE:
    @cargo run --quiet -p pw-cli -- explain {{FILE}}

test-integration:
    @echo "test-integration: not yet applicable (first delivered in Milestone 3)"; exit 1

test-e2e:
    @echo "test-e2e: not yet applicable (first delivered in Milestone 3, Playwright)"; exit 1

test-security:
    @echo "test-security: not yet applicable (first delivered in Milestone 5)"; exit 1

bench:
    @echo "bench: not yet applicable (first delivered in Milestone 3)"; exit 1

demo:
    @echo "demo: not yet applicable (first delivered in Milestone 3)"; exit 1

lab-up:
    @echo "lab-up: not yet applicable (first delivered in Milestone 11)"; exit 1

lab-test:
    @echo "lab-test: not yet applicable (first delivered in Milestone 11)"; exit 1

lab-down:
    @echo "lab-down: not yet applicable (first delivered in Milestone 11)"; exit 1

# ---------------------------------------------------------------------------
# Milestone 0 feasibility spikes (charter §14 M0 task 7)
# ---------------------------------------------------------------------------

# Run all six spikes and write evidence to docs/evidence/E0/.
spikes: spike-compiler-diagnostic spike-koka spike-wasmtime spike-marko spike-layout spike-bonsai

# Every risk-retirement experiment.
rq: rq-resumption rq-row-polymorphism

spike-compiler-diagnostic:
    @bash spikes/compiler-diagnostic/run.sh

spike-koka:
    @bash spikes/koka-js-interop/run.sh

# E2 gate item 4: a domain function executes through GENERATED Koka.
# Needs the pinned toolchain, so it is not part of `just ci`.
spike-pw-to-koka:
    @bash spikes/pw-to-koka/run.sh

# E3: routes GENERATED from `.pw` by `pw emit-marko`, built and measured.
# Needs node + pnpm, so it is not part of `just ci`.
spike-pw-to-marko:
    @bash spikes/pw-to-marko/run.sh

spike-wasmtime:
    @bash spikes/wasmtime-component/run.sh

spike-marko:
    @bash spikes/marko-stream-resume/run.sh

# Charter §7.5A / §14 M0 task 13 — forced-synchronous-layout instrumentation.
spike-layout:
    @bash spikes/layout-phase-scheduler/run.sh

# Charter §14 M0 task 12 — Bonsai/Incremental study.
spike-bonsai:
    @bash spikes/bonsai-incremental-model/run.sh

# ---------------------------------------------------------------------------
# Risk-retirement experiments (see docs/RISK_QUEUE.md)
# These may use temporary dependencies to test assumptions early. Passing one
# retires a risk; it does NOT close the corresponding implementation milestone.
# ---------------------------------------------------------------------------

# RQ-1 — is Marko's resumption real, in Chrome AND Safari?
rq-resumption:
    @bash spikes/browser-resumption/run.sh

# RQ-2 — does Koka propagate effects through higher-order abstraction?
rq-row-polymorphism:
    @bash spikes/koka-row-polymorphism/run.sh

# ---------------------------------------------------------------------------
# CI
# ---------------------------------------------------------------------------

# The one command that must pass for the current milestone's gate.
ci: evidence-gates fmt-check lint case-check mutation-anchors test-unit test-compile mutant-programs
    @echo ""
    @echo "ci: OK"

# For the ARM64 GitHub Actions shard matrix ONLY. This omits exactly the
# workspace-native test recipe; the CI aggregate job must require ALL three
# exhaustive Cargo partitions, and a single passing non-unit job is never
# sufficient evidence of green CI. The normal `just ci` remains unchanged.
ci-without-unit: evidence-gates fmt-check lint case-check mutation-anchors test-compile mutant-programs
    @echo ""
    @echo "ci-without-unit: OK (native Cargo test matrix is REQUIRED separately)"

# ADR-0231: a page's parameter is rendered on every page, and the feed's
# replies. The compiler's tests, the server's, the feed in three engines, and
# the mutation controls.
e14-page-parameters:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0231 - a page's parameter is rendered on every page"; echo; \
       echo "produced by: just e14-page-parameters"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/page_parameters.rs, computed_holes.rs)"; echo; \
       cargo test --locked -p pw-core --test page_parameters --test computed_holes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_page_that_binds_a_query_is_rendered a_row_a_change_renders a_reply_is_its_threads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/page_parameters_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/page_parameters_mutations.py; \
     } > docs/evidence/E14/page-parameters.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/page-parameters.txt

# ADR-0232: an opaque value's representation is read in a template. The
# compiler's tests, the server's, and the mutation controls.
e14-representations:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0232 - an opaque value's representation is read in a template"; echo; \
       echo "produced by: just e14-representations"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/template_representations.rs)"; echo; \
       cargo test --locked -p pw-core --test template_representations 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- an_opaque_values_representation 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== producer controls (scripts/representations_producer_mutations.py)"; echo; \
       python3 scripts/representations_producer_mutations.py; \
       echo; echo "== mutation controls (scripts/template_representations_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/template_representations_mutations.py; \
     } > docs/evidence/E14/representations.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/representations.txt

# ADR-0233: a speculation on a value of a type that contains itself. The
# compiler's tests under Node, the host's through components the compiler
# built, the server's, and the mutation controls.
e14-speculated-graphs:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0233 - a speculation on a value of a type that contains itself"; echo; \
       echo "produced by: just e14-speculated-graphs"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the module (compiler/pw-core/tests/speculated_graphs.rs)"; echo; \
       cargo test --locked -p pw-core --test speculated_graphs 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host (compiler/pw-conformance/tests/browser_graphs.rs)"; echo; \
       cargo test --locked -p pw-conformance --test browser_graphs 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_value_that_contains_itself_is_written 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/speculated_graphs_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/speculated_graphs_mutations.py; \
     } > docs/evidence/E14/speculated-graphs.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/speculated-graphs.txt

# ADR-0234: a view's instance given a speculated value is rendered again with
# it. The compiler's tests, and the mutation controls.
e14-speculated-instances:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0234 - a view's instance given a speculated value is rendered again with it"; echo; \
       echo "produced by: just e14-speculated-instances"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the module (compiler/pw-core/tests/speculated_instances.rs)"; echo; \
       cargo test --locked -p pw-core --test speculated_instances 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/speculated_instances_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/speculated_instances_mutations.py; \
     } > docs/evidence/E14/speculated-instances.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/speculated-instances.txt

# ADR-0235: what a speculated value computes, the page's module computes
# wherever the page shows it. The compiler's tests, the module run under
# Node, and the mutation controls.
e14-speculated-values:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0235 - what a speculated value computes, the page's module computes"; echo; \
       echo "produced by: just e14-speculated-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== the module (compiler/pw-core/tests/speculated_values.rs)"; echo; \
       cargo test --locked -p pw-core --test speculated_values 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/speculated_values_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/speculated_values_mutations.py; \
     } > docs/evidence/E14/speculated-values.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/speculated-values.txt

# ADR-0236: a speculation on the entry a page's parameter keys (ruling
# 0122-d). The compiler's tests, the server's, the feed in three engines, and
# the mutation controls.
e14-speculated-routes:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0236 - a speculation on the entry a page's parameter keys"; echo; \
       echo "produced by: just e14-speculated-routes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/speculated_routes.rs)"; echo; \
       cargo test --locked -p pw-core --test speculated_routes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_speculated_thread_is_named a_speculating_document_carries 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/speculated_routes_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/speculated_routes_mutations.py; \
     } > docs/evidence/E14/speculated-routes.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/speculated-routes.txt

# ADR-0237: what lowering parses, it reports. The checker's tests, the
# parser's, the corpus, and the mutation controls.
e14-read-whole:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0237 - what lowering parses, it reports"; echo; \
       echo "produced by: just e14-read-whole"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/read_whole.rs)"; echo; \
       cargo test --locked -p pw-core --test read_whole 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the parser (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib -- what_a_standalone_parse_leaves a_value_with_no_comma an_optimistic_clause_without 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus (corpus-check, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test checking_source 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/read_whole_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/read_whole_mutations.py; \
     } > docs/evidence/E14/read-whole.txt
    @grep -E "^test result|corpus-check|mutants killed|^---- |panicked at" docs/evidence/E14/read-whole.txt

# ADR-0238: a command speculates on several entries, a page on those it
# shows. The compiler's tests, the parser's, the feed in three engines, and
# the mutation controls.
e14-speculated-arms:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0238 - a command speculates on several entries, a page on those it shows"; echo; \
       echo "produced by: just e14-speculated-arms"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build (compiler/pw-core/tests/speculated_arms.rs)"; echo; \
       cargo test --locked -p pw-core --test speculated_arms 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the parser (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib -- an_optimistic_clause_has_an_arm_for_each_entry 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/speculated_arms_mutations.py)"; echo; \
       python3 scripts/speculated_arms_producer_mutations.py; \
       CARGO_INCREMENTAL=0 python3 scripts/speculated_arms_mutations.py; \
     } > docs/evidence/E14/speculated-arms.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/speculated-arms.txt

# ADR-0239: every code the compiler writes is registered, once. The
# registry's tests, the checker's, the corpus, and the mutation controls.
e14-registered-codes:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0239 - every code the compiler writes is registered, once"; echo; \
       echo "produced by: just e14-registered-codes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the registry (compiler/pw-core/src/codes.rs)"; echo; \
       cargo test --locked -p pw-core --lib codes:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the checker (compiler/pw-core/tests/registered_codes.rs)"; echo; \
       cargo test --locked -p pw-core --test registered_codes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus (corpus-check, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test checking_source 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/registered_codes_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/registered_codes_mutations.py; \
     } > docs/evidence/E14/registered-codes.txt
    @grep -E "^test result|corpus-check|mutants killed|^---- |panicked at" docs/evidence/E14/registered-codes.txt

# ADR-0240: a clause is read once. The checker's tests, the committed store
# graph, the corpus, and the mutation controls.
e14-clauses-read-once:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0240 - a clause is read once"; echo; \
       echo "produced by: just e14-clauses-read-once"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/clauses_read_once.rs)"; echo; \
       cargo test --locked -p pw-core --test clauses_read_once 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the committed store graph (pw-cli)"; echo; \
       cargo test --locked -p pw-cli -- the_committed_graph_matches 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus (corpus-check, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test checking_source 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/clauses_read_once_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/clauses_read_once_mutations.py; \
     } > docs/evidence/E14/clauses-read-once.txt
    @grep -E "^test result|corpus-check|mutants killed|^---- |panicked at" docs/evidence/E14/clauses-read-once.txt

# ADR-0241: an optimistic transition's value is the value typer's. The
# checker's tests, the store's relations, and the mutation controls.
e14-transition-values:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0241 - an optimistic transition's value is the value typer's"; echo; \
       echo "produced by: just e14-transition-values"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/transition_values.rs, policy_term_positions.rs)"; echo; \
       cargo test --locked -p pw-core --test transition_values --test policy_term_positions 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's relations, all decided (value_relations.rs)"; echo; \
       cargo test --locked -p pw-core --test value_relations 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/transition_values_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/transition_values_mutations.py; \
     } > docs/evidence/E14/transition-values.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/transition-values.txt

# ADR-0242: an `{#each}`'s head is read once, by the grammar. The checker's
# tests, the parser's, the readers', and the mutation controls.
e14-each-heads:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0242 - an each head is read once, by the grammar"; echo; \
       echo "produced by: just e14-each-heads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/each_heads.rs)"; echo; \
       cargo test --locked -p pw-core --test each_heads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the parser (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib -- an_each_head_is_its_list 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the readers (every_name_resolves, lexical_scope, each_typing, marko_adapter, template_blocks, keyed, nested_lists)"; echo; \
       cargo test --locked -p pw-core --test every_name_resolves --test lexical_scope --test each_typing --test marko_adapter --test template_blocks --test keyed --test nested_lists 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/each_heads_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/each_heads_mutations.py; \
     } > docs/evidence/E14/each-heads.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/each-heads.txt

# ADR-0243: a block's statements are separated, by `;` or a line. Its tests,
# the corpus's, and the mutation controls.
e14-statements-separated:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0243 - a block's statements are separated, by ; or a line"; echo; \
       echo "produced by: just e14-statements-separated"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/statements_separated.rs)"; echo; \
       cargo test --locked -p pw-core --test statements_separated 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the parser (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib -- two_statements_on_one_line_are_separated 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a clause's head takes its value on its line (compiler/pw-core/src/policy.rs)"; echo; \
       cargo test --locked -p pw-core --lib -- policy::tests::every_clause_head_takes_its_value_on_its_line 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus and the names check (checking_source, every_name_resolves)"; echo; \
       cargo test --locked -p pw-core --test checking_source --test every_name_resolves 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/statements_separated_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/statements_separated_mutations.py; \
     } > docs/evidence/E14/statements-separated.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/statements-separated.txt

# ADR-0247: a clause written in a block is judged by its domain, and a length
# is a CSS length. Its tests, the names check's and the corpus's, and the
# mutation controls.
e14-block-clauses:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0247 - a clause written in a block is judged by its domain"; echo; \
       echo "produced by: just e14-block-clauses"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/block_clauses.rs)"; echo; \
       cargo test --locked -p pw-core --test block_clauses 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a header's values, the names check, the corpus"; echo; \
       cargo test --locked -p pw-core --test policy_values --test every_name_resolves --test statements_separated --test checking_source 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/block_clauses_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/block_clauses_mutations.py; \
     } > docs/evidence/E14/block-clauses.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/block-clauses.txt

# ADR-0248 (ruling 0057-a): a map's key is an Int, a String, a Bool, or an
# opaque type over one, refused at check where it is not. The checker's
# tests, the host's against BTreeMap, the browser's module against the
# component, and the mutation controls. Needs `node`.
e14-map-keys:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0248 - a map's key is ordered, and refused at check where it is not"; echo; \
       echo "produced by: just e14-map-keys"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/map_keys.rs)"; echo; \
       cargo test --locked -p pw-core --test map_keys 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== through the host, against BTreeMap (compiler/pw-conformance/tests/maps.rs)"; echo; \
       cargo test --locked -p pw-conformance --test maps 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser's module against the component (compiler/pw-conformance/tests/javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- --nocapture every_query_agrees_with_its_component_under_node 2>&1 | grep -E '^javascript:|^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/map_keys_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/map_keys_mutations.py; \
     } > docs/evidence/E14/map-keys.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/map-keys.txt

# ADR-0250 (ruling 0099-a): `let _` discards a value, and an acquisition is
# held by a name, ended where it is made, given to the caller, or held by a
# resource's `acquire` clause, or refused.
e14-let-discard:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0250 - let _ discards a value, and an acquisition is held or refused"; echo; \
       echo "produced by: just e14-let-discard"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the parser (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib -- a_let_may_discard_its_value a_discard_is_neither_mutable_nor_a_use 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the checker (compiler/pw-core/tests/let_discard.rs)"; echo; \
       cargo test --locked -p pw-core --test let_discard 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the Koka translation, and a handler (koka_backend.rs, handlers.rs)"; echo; \
       cargo test --locked -p pw-core --test koka_backend -- a_discard_is_kokas_wildcard_val 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --test handlers -- a_handler_that_discards_its_answer 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the generality witnesses (examples/generality/affine_not_consumed_once)"; echo; \
       cargo test --locked -p pw-core --test generality 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/let_discard_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/let_discard_mutations.py; \
     } > docs/evidence/E14/let-discard.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/let-discard.txt

# ADR-0251: a resource's clauses are held to PW2005: what its `acquire` makes
# the resource holds, and what its `release` is given it ends.
e14-resource-clauses:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0251 - a resource's clauses are held to PW2005"; echo; \
       echo "produced by: just e14-resource-clauses"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/resource_clauses.rs)"; echo; \
       cargo test --locked -p pw-core --test resource_clauses 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the generality witnesses (examples/generality/affine_not_consumed_once)"; echo; \
       cargo test --locked -p pw-core --test generality 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== the accepted corpus, A-007, A-019 and A-024 among it, checks as one program"; echo; \
       python3 scripts/accepted_corpus.py; \
       echo; echo "== mutation controls (scripts/resource_clauses_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/resource_clauses_mutations.py; \
     } > docs/evidence/E14/resource-clauses.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/resource-clauses.txt

# ADR-0252: a value returned early carries its label to the caller, with the
# conditions it is returned under.
e14-returned-labels:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0252 - a value returned early carries its label to the caller"; echo; \
       echo "produced by: just e14-returned-labels"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/returned_labels.rs)"; echo; \
       cargo test --locked -p pw-core --test returned_labels 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/returned_labels_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/returned_labels_mutations.py; \
     } > docs/evidence/E14/returned-labels.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/returned-labels.txt

# ADR-0254: a member names the declaration its module sees, and one it cannot
# tell apart is refused (PW0628).
e14-member-resolution:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0254 - a member names the declaration its module sees"; echo; \
       echo "produced by: just e14-member-resolution"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/member_resolution.rs)"; echo; \
       cargo test --locked -p pw-core --test member_resolution 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the member table (compiler/pw-core/src/signatures.rs)"; echo; \
       cargo test --locked -p pw-core --lib -- signatures:: 2>&1 | grep -E '^test result|^---- |panicked at'; \
       echo; echo "== mutation controls (scripts/member_resolution_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/member_resolution_mutations.py; \
     } > docs/evidence/E14/member-resolution.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/member-resolution.txt

# ADR-0255 (ADR-0195's ruling 10): a materialization may read another, the
# build refuses a cycle (PW5109), and invalidation propagates transitively.
e14-materialization-chains:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0255 - a materialization may read another"; echo; \
       echo "produced by: just e14-materialization-chains"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/materialization_chains.rs)"; echo; \
       cargo test --locked -p pw-core --test materialization_chains 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the runtime's reads (runtime/pw-materialize/tests/reads.rs)"; echo; \
       cargo test --locked -p pw-materialize --test reads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/materialization_chains_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/materialization_chains_mutations.py; \
     } > docs/evidence/E14/materialization-chains.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/materialization-chains.txt

# ADR-0256 (ADR-0195's ruling 10): an entry written `_` is every entry at the
# rest, dropped in every session's partition. The PostgreSQL test runs where
# PW_FEED_DATABASE_URL names a database, and passes doing nothing otherwise:
# `e14-feed-postgres` runs it there.
e14-every-entry:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0256 - an entry written with a wildcard is every entry at the rest"; echo; \
       echo "produced by: just e14-every-entry"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $([ -n "${PW_FEED_DATABASE_URL:-}" ] && (psql "$PW_FEED_DATABASE_URL" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)') || echo 'none: PW_FEED_DATABASE_URL is not set')"; echo; \
       echo "== the checker (compiler/pw-core/tests/every_entry.rs)"; echo; \
       cargo test --locked -p pw-core --test every_entry 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the compiled command (compiler/pw-conformance/tests/invalidations.rs)"; echo; \
       cargo test --locked -p pw-conformance --test invalidations 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host (spikes/own-renderer/server/src/tests/every_entry.rs, feed_pg.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- every_entry on_postgres_an_entry_written_with_a_wildcard 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the cache (runtime/pw-resource/tests/concurrency_regressions.rs)"; echo; \
       cargo test --locked -p pw-resource --test concurrency_regressions -- every_entry_named 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/every_entry_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/every_entry_mutations.py; \
     } > docs/evidence/E14/every-entry.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/every-entry.txt

# ADR-0257: the follows timeline. The server's tests (on PostgreSQL where
# PW_FEED_DATABASE_URL names a database, and doing nothing otherwise;
# `e14-feed-postgres` runs them there), the feed in three engines, and the
# mutation controls.
e14-follows:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0257 - the follows timeline"; echo; \
       echo "produced by: just e14-follows"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; \
       echo "postgres: $([ -n "${PW_FEED_DATABASE_URL:-}" ] && (psql "$PW_FEED_DATABASE_URL" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)') || echo 'none: PW_FEED_DATABASE_URL is not set')"; echo; \
       echo "== the server (spikes/own-renderer/server/src/tests/follows.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- tests::follows:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed in three engines (e2e/feed.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/follows_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/follows_mutations.py; \
     } > docs/evidence/E14/follows.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/follows.txt

# ADR-0259 (ruling 0057-c): a map or set from outside is sorted on arrival,
# and a key twice still stops the invocation. The host's tests against
# BTreeMap and BTreeSet, the browser's module against the component, and
# the mutation controls: the sort's, and ADR-0057's, three re-anchored.
# Needs `node`.
e14-arrival:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0259 - a map or set from outside is sorted on arrival"; echo; \
       echo "produced by: just e14-arrival"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== through the host, against BTreeMap and BTreeSet (compiler/pw-conformance/tests/maps.rs)"; echo; \
       cargo test --locked -p pw-conformance --test maps 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser's module against the component (compiler/pw-conformance/tests/javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test javascript -- the_modules_entry_check_is_the_components 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/arrival_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/arrival_mutations.py; \
       echo; echo "== ADR-0057's, three re-anchored (scripts/map_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/map_mutations.py; \
     } > docs/evidence/E14/arrival.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/arrival.txt

# ADR-0262: a host binding is an operation a host provides,
# `"namespace:package/interface#name"`, each part a WIT identifier, held to
# that where it is written (PW0335). The checker's tests and the mutation
# controls.
e14-host-bindings:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0262 - a host binding is an operation a host provides"; echo; \
       echo "produced by: just e14-host-bindings"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/host_bindings.rs)"; echo; \
       cargo test --locked -p pw-core --test host_bindings 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/host_binding_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/host_binding_mutations.py; \
     } > docs/evidence/E14/host-bindings.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/host-bindings.txt

# ADR-0263: a session's, a user's or an organization's handle is the
# platform's to make: constructed by no program (PW5037), answered by no
# data layer (PW5038), supplied by no browser (PW5039). The checker's tests
# and the mutation controls.
e14-handles:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0263 - a handle is the platform's to make"; echo; \
       echo "produced by: just e14-handles"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/handles.rs)"; echo; \
       cargo test --locked -p pw-core --test handles 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/handle_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/handle_mutations.py; \
     } > docs/evidence/E14/handles.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/handles.txt

# ADR-0264: a session's handle never reaches the browser: no query's,
# subscription's or command's answer, and nothing markup prints, holds one
# (PW5040). The checker's tests and the mutation controls.
e14-session-to-browser:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0264 - a session's handle never reaches the browser"; echo; \
       echo "produced by: just e14-session-to-browser"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/session_to_browser.rs)"; echo; \
       cargo test --locked -p pw-core --test session_to_browser 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/session_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/session_mutations.py; \
     } > docs/evidence/E14/session-to-browser.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/session-to-browser.txt

# ADR-0265: a form goes where something answers it: its action a route that
# answers its method (PW5041), the relying party's routes among them, and a
# link a route that answers a get. The checker's tests, the server's, and
# the mutation controls.
e14-form-routes:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0265 - a form goes where something answers it"; echo; \
       echo "produced by: just e14-form-routes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the checker (compiler/pw-core/tests/form_routes.rs)"; echo; \
       cargo test --locked -p pw-core --test form_routes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host's routes (spikes/own-renderer/server/src/tests/sign_in.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- the_relying_party_answers 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/form_route_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/form_route_mutations.py; \
     } > docs/evidence/E14/form-routes.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/form-routes.txt

# ADR-0266: an export's parameters past the flat limit arrive in memory, a
# tuple of them at one pointer, each held where it sits. Through the host,
# and the mutation controls.
e14-wide-parameters:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0266 - an export's parameters past the flat limit arrive in memory"; echo; \
       echo "produced by: just e14-wide-parameters"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== through the host (compiler/pw-conformance/tests/wide_parameters.rs)"; echo; \
       cargo test --locked -p pw-conformance --test wide_parameters 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/wide_parameter_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/wide_parameter_mutations.py; \
     } > docs/evidence/E14/wide-parameters.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/wide-parameters.txt

# ADR-0268: a command outlives the page that sent it. Its request kept alive
# within the Fetch standard's 64 KiB, in three engines, and the mutation
# controls.
e14-keepalive:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0268 - a command outlives the page that sent it"; echo; \
       echo "produced by: just e14-keepalive"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== kept alive, in three engines (e2e/keepalive.spec.mjs; e2e/feed.spec.mjs, a post by its bytes)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/keepalive.spec.mjs --reporter=line; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs -g "kept alive, counted by its bytes" --reporter=line; \
       echo; echo "== mutation controls (scripts/keepalive_mutations.py)"; echo; \
       python3 scripts/keepalive_mutations.py; \
     } > docs/evidence/E14/keepalive.txt
    @grep -E "passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/keepalive.txt

# ADR-0302: a refusal is told where the press was. The checker's tests, the
# host's, the browser's in three engines, and the mutation controls.
e14-refusal:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @BUILD_ONLY=1 bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0302 - a refusal is told where the press was"; echo; \
       echo "produced by: just e14-refusal"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== a predicate says what a refusal by it is told (compiler/pw-core, PW0351)"; echo; \
       cargo test --locked -p pw-core --test predicates 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the build's page holds the announcer (runtime/pw-render, tests/titles.rs)"; echo; \
       cargo test --locked -p pw-render --test titles 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host answers a refusal with its words, kept with its press; a page holds its announcer"; echo; \
       cargo test --locked -p pw-dev-server -- sign_in:: a_signal_page_holds_its_announcer 2>&1 \
         | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== told where the press was, in three engines (e2e/identity.spec.mjs; recovery, lazy-handler, accessibility)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/identity.spec.mjs --reporter=line; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/recovery.spec.mjs -g "still stale" --reporter=line; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/lazy-handler.spec.mjs -g "fails to load" --reporter=line; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/accessibility.spec.mjs -g "announcer|same node from the start" --reporter=line; \
       echo; echo "== mutation controls, part 1 of 2 (scripts/refusal_mutations.py --part 1/2; part 2 is e14-refusal-2's)"; echo; \
       python3 scripts/refusal_mutations.py --part 1/2; \
     } > docs/evidence/E14/refusal.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/refusal.txt

# ADR-0302's mutation controls, part 2 of 2 (the integrator's ruling 1 of
# 2026-10-17, train-10): the browser mutants `e14-refusal`'s job has no time
# for, run in a job of their own. Its builds, and the controls.
e14-refusal-2:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @BUILD_ONLY=1 bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0302 - a refusal is told where the press was: its mutation controls, part 2 of 2"; echo; \
       echo "produced by: just e14-refusal-2"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== mutation controls, part 2 of 2 (scripts/refusal_mutations.py --part 2/2; part 1 is e14-refusal's)"; echo; \
       python3 scripts/refusal_mutations.py --part 2/2; \
     } > docs/evidence/E14/refusal-2.txt
    @grep -E "passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/refusal-2.txt

# ADR-0305: a page's commands run in the order it sent them. The server's
# tests, the browser's in three engines, and the mutation controls.
e14-command-order:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0305 - a page's commands run in the order it sent them"; echo; \
       echo "produced by: just e14-command-order"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== a command runs once the one its page sent before it has (server/src/order.rs, the command route)"; echo; \
       cargo test --locked -p pw-dev-server -- order:: a_pages_command a_session_forgotten_takes_its_commands_order 2>&1 \
         | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== in three engines (e2e/command-order.spec.mjs; optimistic.spec, navigate.spec)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/command-order.spec.mjs --reporter=line; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/optimistic.spec.mjs e2e/navigate.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/command_order_mutations.py)"; echo; \
       python3 scripts/command_order_mutations.py; \
     } > docs/evidence/E14/command-order.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/command-order.txt

# ADR-0269: a resource is held by what takes it apart. The checker's tests,
# and the mutation controls.
e14-matched-resources:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0269 - a resource is held by what takes it apart"; echo; \
       echo "produced by: just e14-matched-resources"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== taken apart (compiler/pw-core/tests/matched_resources.rs, let_discard.rs)"; echo; \
       cargo test --locked -p pw-core --test matched_resources --test let_discard 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/matched_resource_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/matched_resource_mutations.py; \
     } > docs/evidence/E14/matched-resources.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/matched-resources.txt

# ADR-0267: a component's trap says why it stopped. Each cause through the
# host, the browser's module agreeing by cause, and the mutation controls.
e14-trap-causes:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0267 - a component's trap says why it stopped"; echo; \
       echo "produced by: just e14-trap-causes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== each cause through the host (compiler/pw-conformance/tests/trap_causes.rs)"; echo; \
       cargo test --locked -p pw-conformance --test trap_causes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== nodes that are no tree, by name (compiler/pw-conformance/tests/recursive_types.rs)"; echo; \
       cargo test --locked -p pw-conformance --test recursive_types 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the browser's module and the component agree by cause (compiler/pw-conformance/tests/javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test javascript 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/trap_cause_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/trap_cause_mutations.py; \
     } > docs/evidence/E14/trap-causes.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/trap-causes.txt

# ADR-0271: a reader is told once per burst, and served in turn. The
# development server's tests, and the mutation controls.
e14-telling:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0271 - a reader is told once per burst, and served in turn"; echo; \
       echo "produced by: just e14-telling"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== told once, in turn (spikes/own-renderer/server/src/main.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- a_burst_of_posts_tells_another_reader_once \
         a_commit_while_a_reader_is_told_is_told_after_it a_telling_that_panics_does_not_silence_the_reader \
         a_sessions_turns_are_served_in_the_order_asked a_post_reaches_every_open_timeline 2>&1 \
         | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/telling_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/telling_mutations.py; \
     } > docs/evidence/E14/telling.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/telling.txt

# ADR-0272: a region the browser fills while the runtime boots is bound. In
# the host's Chrome, and the mutation controls.
e14-stream-boot:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0272 - a region the browser fills while the runtime boots is bound"; echo; \
       echo "produced by: just e14-stream-boot"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== in the host's Chrome (e2e/stream.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/stream.spec.mjs \
          -g "while the runtime boots|Chrome 150 and later" --project=chromium --reporter=line; \
       echo; echo "== mutation controls (scripts/stream_boot_mutations.py)"; echo; \
       python3 scripts/stream_boot_mutations.py; \
     } > docs/evidence/E14/stream-boot.txt
    @grep -E "passed|failed|skipped|mutants killed|Error:" docs/evidence/E14/stream-boot.txt

# ADR-0273: a materialization is a value its body derives. The checker's
# tests, and the mutation controls.
e14-materialization-bodies:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0273 - a materialization is a value its body derives"; echo; \
       echo "produced by: just e14-materialization-bodies"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== derived, read and stated once (compiler/pw-core/tests/materialization_bodies.rs, calls_name_terms.rs)"; echo; \
       cargo test --locked -p pw-core --test materialization_bodies --test calls_name_terms 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/materialization_body_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/materialization_body_mutations.py; \
     } > docs/evidence/E14/materialization-bodies.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/materialization-bodies.txt

# ADR-0276: `pw fmt` changes no program's meaning. The lexer's, the
# formatter's and the backend's tests, every program held to `pw fmt
# --check`, and the mutation controls.
e14-fmt-meaning:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0276 - pw fmt changes no program's meaning"; echo; \
       echo "produced by: just e14-fmt-meaning"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the lexer and the formatter (compiler/pw-syntax)"; echo; \
       cargo test --locked -p pw-syntax 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a number's grouped digits (compiler/pw-core/tests/backend_lowering.rs)"; echo; \
       cargo test --locked -p pw-core --test backend_lowering 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== every program held to pw fmt --check"; echo; \
       cargo run --quiet -p pw-cli -- fmt --check examples/*.pw examples/lib/*.pw examples/accepted/*.pw examples/rejected/*.pw examples/rules/*/*.pw examples/kiokun/*.pw examples/kiokun-site/*.pw packages/*/*.pw examples/feed/*.pw examples/store/*.pw examples/demo/*.pw examples/generality/*/*.pw 2>&1; \
       echo; echo "== mutation controls (scripts/fmt_meaning_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/fmt_meaning_mutations.py; \
     } > docs/evidence/E14/fmt-meaning.txt
    @grep -E "^test result|already formatted|need formatting|mutants killed|^---- |panicked at" docs/evidence/E14/fmt-meaning.txt

# ADR-0280: a handler navigates after its command commits. The compiler's
# tests, the store's order placed and gone to in three engines
# (e2e/navigate.spec.mjs), and the mutation controls.
e14-navigate:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0280 - a handler navigates after its command commits"; echo; \
       echo "produced by: just e14-navigate"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/navigate.rs)"; echo; \
       cargo test --locked -p pw-core --test navigate 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the order placed and gone to, in three engines (e2e/navigate.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/navigate.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/navigate_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/navigate_mutations.py; \
     } > docs/evidence/E14/navigate.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/navigate.txt

# ADR-0277: a materialization is kept, and a page reads it. The compiler's,
# the materializer's and the server's tests, the store's chain in three
# engines (e2e/materialized.spec.mjs), and the mutation controls.
e14-materializations-kept:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0277 - a materialization is kept, and a page reads it"; echo; \
       echo "produced by: just e14-materializations-kept"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/materializations_kept.rs, materialization_bodies.rs)"; echo; \
       cargo test --locked -p pw-core --test materializations_kept --test materialization_bodies 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the chain's order (runtime/pw-materialize/tests/chain_order.rs)"; echo; \
       cargo test --locked -p pw-materialize --test chain_order 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server (materializations.rs, tests/materializations.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- materializations 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's chain in three engines (e2e/materialized.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/materialized.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/materializations_kept_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/materializations_kept_mutations.py; \
     } > docs/evidence/E14/materializations-kept.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/materializations-kept.txt

# ADR-0282: what a value holds is one label. The compiler's tests, the
# rejected corpus's exhibits through `checking_source`, and the mutation
# controls.
e14-held-labels:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0282 - what a value holds is one label"; echo; \
       echo "produced by: just e14-held-labels"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the compiler (held_where_it_runs.rs, reads_through_calls.rs, checking_source.rs)"; echo; \
       cargo test --locked -p pw-core --test held_where_it_runs --test reads_through_calls --test checking_source 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/held_labels_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/held_labels_mutations.py; \
     } > docs/evidence/E14/held-labels.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/held-labels.txt


# ADR-0283 and ADR-0284, from W6's report on kiokun's word page: a
# component's contract is what its code does, and a value of any type is
# fixed by the call that meets it. The compiler's tests, the store's
# contracts and worlds held to the committed ones, and the mutation controls.
e14-what-a-component-does:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0283 and ADR-0284 - a component's contract is what its code does; a value of any type is fixed by the call that meets it"; echo; \
       echo "produced by: just e14-what-a-component-does"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the compiler (what_a_component_does.rs, any_type.rs, component_contract.rs, evidence_is_current.rs)"; echo; \
       cargo test --locked -p pw-core --test what_a_component_does --test any_type --test component_contract --test evidence_is_current 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/what_a_component_does_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/what_a_component_does_mutations.py; \
     } > docs/evidence/E14/what-a-component-does.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/what-a-component-does.txt

# ADR-0287: a handler is held where it runs. The compiler's tests and the
# mutation controls.
e14-handlers-where-they-run:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0287 - a handler is held where it runs"; echo; \
       echo "produced by: just e14-handlers-where-they-run"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the compiler (handlers_where_they_run.rs, handlers_in_the_browser.rs)"; echo; \
       cargo test --locked -p pw-core --test handlers_where_they_run --test handlers_in_the_browser 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/handlers_where_they_run_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/handlers_where_they_run_mutations.py; \
     } > docs/evidence/E14/handlers-where-they-run.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/handlers-where-they-run.txt

# ADR-0290: a verification run is dealt by the seconds its recipes last
# took. The CI scripts' tests and the mutation controls.
e14-dealt-by-time:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0290 - a verification run is dealt by the seconds its recipes last took"; echo; \
       echo "produced by: just e14-dealt-by-time"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "python: $(python3 --version)"; echo; \
       echo "== the plan and the fetch (scripts/tests/test_ci_scripts.py)"; echo; \
       python3 -m unittest -v scripts/tests/test_ci_scripts.py 2>&1 | grep -E ' \.\.\. |^Ran |^OK|^FAILED'; \
       echo; echo "== mutation controls (scripts/dealt_by_time_mutations.py)"; echo; \
       python3 scripts/dealt_by_time_mutations.py; \
     } > docs/evidence/E14/dealt-by-time.txt
    @grep -E "^Ran |^OK|^FAILED|mutants killed" docs/evidence/E14/dealt-by-time.txt

# ADR-0292: a mutation script's processes are bounded in memory. The bound's
# tests and `mutation_baseline`'s, which starts it, and the mutation
# controls.
e14-mutation-bound:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0292 - a mutation script's processes are bounded in memory"; echo; \
       echo "produced by: just e14-mutation-bound"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "python: $(python3 --version)"; echo "platform: $(uname -sm)"; echo; \
       echo "== the bound (scripts/tests/test_mutation_bound.py, test_mutation_baseline.py)"; echo; \
       python3 -m unittest -v scripts/tests/test_mutation_bound.py scripts/tests/test_mutation_baseline.py 2>&1 | grep -E ' \.\.\. |^Ran |^OK|^FAILED'; \
       echo; echo "== mutation controls (scripts/mutation_bound_mutations.py)"; echo; \
       python3 scripts/mutation_bound_mutations.py; \
     } > docs/evidence/E14/mutation-bound.txt
    @grep -E "^Ran |^OK|^FAILED|mutants killed" docs/evidence/E14/mutation-bound.txt

# ADR-0294: a list renders in its length (W6's finding on kiokun's sample).
# The renderer's scope test, the list's scale timed, and the mutation controls.
e14-shared-scopes:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0294 - a list renders in its length"; echo; \
       echo "produced by: just e14-shared-scopes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the renderer (runtime/pw-render: scopes, tests/list_scale.rs)"; echo; \
       cargo test --locked -p pw-render --lib scopes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-render --test list_scale -- --include-ignored --nocapture 2>&1 | grep -E '^(test |test result)|names|ratio|panicked at'; \
       echo; echo "== mutation controls (scripts/shared_scopes_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/shared_scopes_mutations.py; \
     } > docs/evidence/E14/shared-scopes.txt
    @grep -E "^test result|mutants killed|^---- |panicked at|ratio" docs/evidence/E14/shared-scopes.txt

# ADR-0295: a page at another address of the page is moved there. The
# compiler's tests, the development server's, and the mutation controls.
e14-redirects:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0295 - a page at another address of the page is moved there"; echo; \
       echo "produced by: just e14-redirects"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the compiler (compiler/pw-core/tests/redirects.rs, not_found.rs)"; echo; \
       cargo test --locked -p pw-core --test redirects --test not_found 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the development server (spikes/own-renderer/server/src/tests/redirects.rs)"; echo; \
       cargo test --locked -p pw-dev-server tests::redirects 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/redirects_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/redirects_mutations.py; \
     } > docs/evidence/E14/redirects.txt
    @grep -E "^test result|mutants killed|^---- |panicked at" docs/evidence/E14/redirects.txt

# ADR-0296: a change is derived outside the table, and what reaches a
# document is recorded. The development server's tests, the baseline's
# records, and the mutation controls.
e14-derived-outside:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0296 - a change is derived outside the table, and what reaches a document is recorded"; echo; \
       echo "produced by: just e14-derived-outside"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "python: $(python3 --version)"; echo; \
       echo "== the development server (the table while a change derives, the attempts, the trail)"; echo; \
       cargo test --locked -p pw-dev-server -- the_table_is_free a_document_changed a_documents_trail a_keyed_read_whose_document_changed 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a red baseline's records (scripts/tests/test_mutation_baseline.py)"; echo; \
       python3 -m unittest -v scripts/tests/test_mutation_baseline.py 2>&1 | grep -E ' \.\.\. |^Ran |^OK|^FAILED'; \
       echo; echo "== mutation controls (scripts/derived_outside_mutations.py)"; echo; \
       python3 scripts/derived_outside_mutations.py; \
     } > docs/evidence/E14/derived-outside.txt
    @grep -E "^test result|^Ran |^OK|^FAILED|mutants killed|panicked at" docs/evidence/E14/derived-outside.txt

# ADR-0297: a commit tells the pages that ask, and tells them at once. The
# development server's tests, and the mutation controls.
e14-tell-at-once:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0297 - a commit tells the pages that ask, and tells them at once"; echo; \
       echo "produced by: just e14-tell-at-once"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the development server (readers told at once; a document passed by, told as its page asks)"; echo; \
       cargo test --locked -p pw-dev-server -- a_reader_is_told_while a_document_no_page_asks_for a_telling_that_panics a_burst 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/tell_at_once_mutations.py)"; echo; \
       python3 scripts/tell_at_once_mutations.py; \
     } > docs/evidence/E14/tell-at-once.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/tell-at-once.txt

# ADR-0300: a build is named by what it built, and the browser's decision
# holds a document to what its build says of its page. The compiler's tests,
# the host's, the decision's, the browser's in three engines, and the
# mutation controls.
e14-build-id:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0300 - a build is named by what it built"; echo; \
       echo "produced by: just e14-build-id"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the build's name (compiler/pw-core/tests/build_id.rs)"; echo; \
       cargo test --locked -p pw-core --test build_id 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host serves it, and each page's document"; echo; \
       cargo test --locked -p pw-dev-server -- a_host_serves_its_build a_build_that_names_no_build 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the decision holds a document to its page (runtime/pw-resume-wasm)"; echo; \
       cargo test --locked -p pw-resume-wasm 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a document of another schema, in three engines (e2e/recovery.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/recovery.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/build_id_mutations.py)"; echo; \
       python3 scripts/build_id_mutations.py; \
     } > docs/evidence/E14/build-id.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/build-id.txt

# ADR-0303: a page is shown in its layout, which the pages that name it
# share. The compiler's tests, the host's, the store's pages in three
# engines, and the mutation controls.
e14-layouts:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0303 - a page is shown in its layout"; echo; \
       echo "produced by: just e14-layouts"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the layout, composed into each page (compiler/pw-core/tests/layouts.rs)"; echo; \
       cargo test --locked -p pw-core --test layouts 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's pages, served in their layout"; echo; \
       cargo test --locked -p pw-dev-server -- a_store_page_is_shown_in_its_layout_and_its_count_is_told 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the store's pages in three engines (e2e/layouts.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/layouts.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/layouts_mutations.py)"; echo; \
       python3 scripts/layouts_mutations.py; \
     } > docs/evidence/E14/layouts.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/layouts.txt

# ADR-0307: a control is shown where its command's predicates hold. The
# checker's, the lowering's and the plan's tests, the host's, the feed and
# the store in three engines, and the mutation controls.
e14-holds:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @BUILD_ONLY=1 bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0307 - a control is shown where its command's predicates hold"; echo; \
       echo "produced by: just e14-holds"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== what a template reads, where a control stands, and the plan (compiler/pw-core/tests/controls_where_they_hold.rs)"; echo; \
       cargo test --locked -p pw-core --test controls_where_they_hold 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host answers what a page asks of its reader"; echo; \
       cargo test --locked -p pw-dev-server -- sign_in:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the feed and the store, in three engines (e2e/feed.spec.mjs, e2e/identity.spec.mjs, e2e/store.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs e2e/identity.spec.mjs e2e/store.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/holds_mutations.py)"; echo; \
       python3 scripts/holds_mutations.py; \
     } > docs/evidence/E14/holds.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/holds.txt

# ADR-0309: a page's query parameters are typed, and a GET form makes them.
# The checker's tests, the host's, the demo's search in three engines with
# scripts on and off, and the mutation controls.
e14-query-params:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0309 - a page's query parameters are typed, and a GET form makes them"; echo; \
       echo "produced by: just e14-query-params"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== a route's query part, and what an address gives (compiler/pw-core/tests/routes.rs, uploads.rs)"; echo; \
       cargo test --locked -p pw-core --test routes --test uploads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host reads a query as a browser's form writes it, and moves a page by its path"; echo; \
       cargo test --locked -p pw-dev-server -- a_query_is_read_as_a_browsers_form_writes_it a_route_with_a_query_part_is_moved_by_its_path 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the demo's search, in three engines (e2e/query.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/query.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/query_params_mutations.py)"; echo; \
       python3 scripts/query_params_mutations.py; \
     } > docs/evidence/E14/query-params.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/query-params.txt

# The soft navigation's ADR: a navigation keeps the layout, and shows the next
# page in it. The browser's tests in three engines, ADR-0280's navigation as
# it is now, and the mutation controls.
e14-soft-navigation:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0306 - a navigation keeps the layout, and shows the next page in it"; echo; \
       echo "produced by: just e14-soft-navigation"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the store's pages, followed softly, in three engines (e2e/soft-navigation.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/soft-navigation.spec.mjs --reporter=line; \
       echo; echo "== a handler's navigation after its commit, in three engines (e2e/navigate.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/navigate.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/soft_navigation_mutations.py)"; echo; \
       python3 scripts/soft_navigation_mutations.py; \
     } > docs/evidence/E14/soft-navigation.txt
    @grep -E "passed|failed|mutants killed|Error:" docs/evidence/E14/soft-navigation.txt

# ADR-0311: a page states its canonical address, and a navigation takes the
# next page's head whole. Where a canonical link is and what it addresses,
# the head it is written into, the host's origin, the corpus at C19, the
# browser in three engines, and the mutation controls.
e14-canonical:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0311 - a page states its canonical address"; echo; \
       echo "produced by: just e14-canonical"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== where a canonical link is, and what it addresses (compiler/pw-core/tests/canonical.rs, links.rs)"; echo; \
       cargo test --locked -p pw-core --test canonical --test links 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the head it is written into (runtime/pw-render/tests/canonical.rs)"; echo; \
       cargo test --locked -p pw-render --test canonical 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host: the deployment's origin, and an origin read as one"; echo; \
       cargo test --locked -p pw-dev-server -- a_page_s_canonical_address_is_written_with_the_deployment_s_origin an_origin_is_a_scheme_a_host_and_a_port 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the corpus at C19 (corpus-check, generality.rs, checking_source.rs)"; echo; \
       cargo run --quiet --locked -p corpus-check -- examples 2>&1 | tail -5; \
       cargo test --locked -p pw-core --test generality --test checking_source 2>&1 | grep -E '^test result|panicked at'; \
       echo; echo "== a canonical address, and a navigation's head, in three engines (e2e/canonical.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/canonical.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/canonical_mutations.py)"; echo; \
       python3 scripts/canonical_mutations.py; \
     } > docs/evidence/E14/canonical.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at|corpus-check" docs/evidence/E14/canonical.txt

# ADR-0312: a page and its layout show one entry by one value. The
# speculation module's binding and its alias, run under Node, the host's one
# entry, the layout's count in three engines, and the mutation controls.
e14-one-entry:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0312 - a page and its layout show one entry by one value"; echo; \
       echo "produced by: just e14-one-entry"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the store's page's module: one binding, its layout's name an alias (compiler/pw-core/tests/layouts.rs)"; echo; \
       cargo test --locked -p pw-core --test layouts 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the module run under Node (compiler/pw-conformance/tests/speculation.rs)"; echo; \
       cargo test --locked -p pw-conformance --test speculation 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host: one entry for the store's cart"; echo; \
       cargo test --locked -p pw-dev-server -- a_page_and_its_layout_show_one_entry_by_one_value 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the layout's count, in three engines (e2e/layouts.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/layouts.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/one_entry_mutations.py)"; echo; \
       python3 scripts/one_entry_mutations.py; \
     } > docs/evidence/E14/one-entry.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/one-entry.txt

# ADR-0317: a component is compiled once a process. The host's cache
# (runtime/pw-host/tests/compiled_once.rs, the engine's proposals), the
# conformance harness that reads a component's imports from it, the
# development server's suite whole at four tests at once against ADR-0292's
# bound (scripts/suite_memory.py), and the mutation controls.
e14-compiled-once:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0317 - a component is compiled once a process"; echo; \
       echo "produced by: just e14-compiled-once"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== compiled once, its imports read from it (runtime/pw-host/tests/compiled_once.rs, engine_proposals.rs)"; echo; \
       cargo test --locked -p pw-host --features engine --test compiled_once --test engine_proposals 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the conformance harness (compiler/pw-conformance)"; echo; \
       cargo test --locked -p pw-conformance 2>&1 | grep -E '^test result|panicked at'; \
       echo; echo "== the development server's suite, whole, four tests at once (scripts/suite_memory.py)"; echo; \
       python3 scripts/suite_memory.py --threads 4; \
       echo; echo "== mutation controls (scripts/compiled_once_mutations.py)"; echo; \
       python3 scripts/compiled_once_mutations.py; \
     } > docs/evidence/E14/compiled-once.txt
    @grep -E "^test result|mutants killed|panicked at|peak resident|within the bound|OVER THE BOUND|^wall" docs/evidence/E14/compiled-once.txt
# ADR-0324: `private` says a declaration is not importable, and nothing more
# (NEXT 3a). A module's own helper is not the user's data, a `private`
# declaration is not imported, and `private` on one that holds data is
# refused by name (compiler/pw-core/tests/private_is_not_importable.rs,
# user_visibility.rs); a `user` that opens a declaration after one with no
# body is its own (pw-syntax); and the mutation controls
# (scripts/private_module_mutations.py).
e14-private-module:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0324 - private says a declaration is not importable, and nothing more"; echo; \
       echo "produced by: just e14-private-module"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the ruling (compiler/pw-core/tests/private_is_not_importable.rs, user_visibility.rs)"; echo; \
       cargo test --locked -p pw-core --test private_is_not_importable --test user_visibility 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a user declaration after one with no body (compiler/pw-syntax)"; echo; \
       cargo test --locked -p pw-syntax --lib a_user_declaration_after_one_with_no_body_is_its_own 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/private_module_mutations.py)"; echo; \
       python3 scripts/private_module_mutations.py; \
     } > docs/evidence/E14/private-module.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/private-module.txt


# ADR-0321: a related note is shown in its own file. The checker's notes in
# another file name it (compiler/pw-core/tests/related_files.rs); `pw check`
# shows each in its own file, and never stops on a span (pw-cli's tests and
# tests/related_files.rs, W8's program); and the mutation controls.
e14-related-files:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0321 - a related note is shown in its own file"; echo; \
       echo "produced by: just e14-related-files"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== each note in its own file (compiler/pw-core/tests/related_files.rs)"; echo; \
       cargo test --locked -p pw-core --test related_files 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== pw check: a note shown in its own file, and no stop on a span (compiler/pw-cli)"; echo; \
       cargo test --locked -p pw-cli 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/related_files_mutations.py)"; echo; \
       python3 scripts/related_files_mutations.py; \
     } > docs/evidence/E14/related-files.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/related-files.txt

# ADR-0327: money is rounded only where a program names how (NEXT 6a). Each
# rounding, compiled to a component and to JavaScript, agreeing, at the
# amounts each rounding says (compiler/pw-conformance/tests/javascript.rs),
# and the mutation controls (scripts/money_mutations.py).
e14-money:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0327 - money is rounded only where a program names how"; echo; \
       echo "produced by: just e14-money"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; echo; \
       echo "== each rounding, both ways (compiler/pw-conformance/tests/javascript.rs)"; echo; \
       cargo test --locked -p pw-conformance --test javascript money_is_rounded_as_the_program_names_it 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/money_mutations.py)"; echo; \
       python3 scripts/money_mutations.py; \
     } > docs/evidence/E14/money.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/money.txt

# ADR-0326: a name imported by name is the file's, before a whole module's.
# The resolution's order and the refusal of a call two whole modules declare
# (compiler/pw-core/tests/explicit_imports.rs), every program checking as
# before (`just test-compile`), and the mutation controls
# (scripts/explicit_imports_mutations.py).
e14-explicit-imports:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0326 - a name imported by name is the file's, before a whole module's"; echo; \
       echo "produced by: just e14-explicit-imports"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the order, and the refusal (compiler/pw-core/tests/explicit_imports.rs)"; echo; \
       cargo test --locked -p pw-core --test explicit_imports 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/explicit_imports_mutations.py)"; echo; \
       python3 scripts/explicit_imports_mutations.py; \
     } > docs/evidence/E14/explicit-imports.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/explicit-imports.txt

# ADR-0330: an arm's binding is read through a member, as a block's value is
# (the integrator's; W9's finding in NEXT 6b). The plan's tests
# (compiler/pw-core/tests/arm_member_reads.rs), the host's payload
# (payload_tests), and the mutation controls
# (scripts/arm_member_reads_mutations.py).
e14-arm-member-reads:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0330 - an arm's binding is read through a member, as a block's value is"; echo; \
       echo "produced by: just e14-arm-member-reads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the plan (compiler/pw-core/tests/arm_member_reads.rs)"; echo; \
       cargo test --locked -p pw-core --test arm_member_reads 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host's payload (spikes/own-renderer/server/src/main.rs, payload_tests)"; echo; \
       cargo test --locked -p pw-dev-server -- payload_tests 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/arm_member_reads_mutations.py)"; echo; \
       python3 scripts/arm_member_reads_mutations.py; \
     } > docs/evidence/E14/arm-member-reads.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/arm-member-reads.txt

# ADR-0331: a case crosses from a browser (the integrator's; W9's finding in
# NEXT 6b). The handler sending a case and the host reading it
# (compiler/pw-conformance/tests/case_arguments.rs), and the mutation
# controls (scripts/case_arguments_mutations.py).
e14-case-arguments:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0331 - a case crosses from a browser"; echo; \
       echo "produced by: just e14-case-arguments"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== a case sent and read (compiler/pw-conformance/tests/case_arguments.rs)"; echo; \
       cargo test --locked -p pw-conformance --test case_arguments 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/case_arguments_mutations.py)"; echo; \
       python3 scripts/case_arguments_mutations.py; \
     } > docs/evidence/E14/case-arguments.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/case-arguments.txt

# ADR-0322: `|` and `&` are parenthesized where they mix (W9's finding). The
# parser's rule (compiler/pw-syntax/src/grammar.rs), every shipped program and
# the corpus parsing as before, and the mutation controls
# (scripts/logical_mixed_mutations.py).
e14-logical-mixed:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0322 - | and & are parenthesized where they mix"; echo; \
       echo "produced by: just e14-logical-mixed"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the parser's rule (compiler/pw-syntax/src/grammar.rs)"; echo; \
       cargo test --locked -p pw-syntax --lib 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== every shipped program and the corpus parse as before (pw-syntax's corpus tests)"; echo; \
       cargo test --locked -p pw-syntax --test corpus_grammar --test corpus_lossless 2>&1 | grep -E '^test result|panicked at'; \
       echo; echo "== mutation controls (scripts/logical_mixed_mutations.py)"; echo; \
       python3 scripts/logical_mixed_mutations.py; \
     } > docs/evidence/E14/logical-mixed.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/logical-mixed.txt

# ADR-0320: a block is told part by part while its arm stays. The plan names
# each block at the top of a page that renders a query's value, whatever
# decides it (compiler/pw-core/tests/blocks_that_render_a_query.rs); a host
# tells one part by part while it shows the arm it showed, as a row is
# (runtime/pw-render/tests/block_changes.rs, the host's tests); W9's search
# sentence, inside its `{#match q}` arm, is told where it is, its status
# region kept, in three engines (e2e/store-search.spec.mjs); and the mutation
# controls (scripts/block_parts_mutations.py).
e14-block-parts:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0320 - a block is told part by part while its arm stays"; echo; \
       echo "produced by: just e14-block-parts"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the blocks a page's plan names (compiler/pw-core/tests/blocks_that_render_a_query.rs, page_blocks.rs)"; echo; \
       cargo test --locked -p pw-core --test blocks_that_render_a_query --test page_blocks 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what changed in a block, and in a row, part by part (runtime/pw-render/tests/block_changes.rs, instance_changes.rs)"; echo; \
       cargo test --locked -p pw-render --test block_changes --test instance_changes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host: a block told part by part, one a speculation renders, and the search's sentence"; echo; \
       cargo test --locked -p pw-dev-server -- a_block_that_shows_the_arm_it_showed_is_told_part_by_part a_block_a_speculation_renders_is_rendered_again_whole a_block_whose_rendering_changed_is_rendered_again the_sentence_inside_its_arm_is_told_where_it_is an_open_search_page_is_told_of_a_menu_change 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the search's sentence told where it is, in three engines (e2e/store-search.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/store-search.spec.mjs \
          -g "a dish added while the page is open reaches it" --reporter=line; \
       echo; echo "== mutation controls (scripts/block_parts_mutations.py)"; echo; \
       python3 scripts/block_parts_mutations.py; \
     } > docs/evidence/E14/block-parts.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/block-parts.txt

# ADR-0323: a member read is computed wherever a page reads it (NEXT 3c). The
# plan's member reads inside a block, in an attribute, a block's subject, the
# title and the metadata, and in a speculated block
# (compiler/pw-core/tests/member_reads_in_blocks.rs); the host's count inside
# a block, told where it is; the store's checkout link, its subtotal shown
# before the server answers, in three engines (e2e/cart.spec.mjs); and the
# mutation controls (scripts/member_reads_mutations.py).
e14-member-reads:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0323 - a member read is computed wherever a page reads it"; echo; \
       echo "produced by: just e14-member-reads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the plan's member reads (compiler/pw-core/tests/member_reads_in_blocks.rs)"; echo; \
       cargo test --locked -p pw-core --test member_reads_in_blocks 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host: a count inside a block, told where it is, and in a block a speculation renders"; echo; \
       cargo test --locked -p pw-dev-server -- a_member_read_inside_a_block_is_computed_and_told_where_it_is a_member_read_inside_a_speculated_block_is_rendered_with_it 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the checkout link's subtotal before the server answers, in three engines (e2e/cart.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/cart.spec.mjs \
          -g "a line shows before the server answers|remove takes the line away" --reporter=line; \
       echo; echo "== mutation controls (scripts/member_reads_mutations.py)"; echo; \
       python3 scripts/member_reads_mutations.py; \
     } > docs/evidence/E14/member-reads.txt
    @grep -E "^test result|passed|failed|mutants killed|Error:|panicked at" docs/evidence/E14/member-reads.txt
