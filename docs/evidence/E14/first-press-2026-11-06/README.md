# The first press: boot preloads and production warming, measured (2026-11-06)

Evidence for the first press's ADR
(`docs/DECISIONS/ADR-XXXX-the-first-press-finds-its-code.md`). Local,
exploratory A/B on the development PC; not the full `just e14-head-to-head`
acceptance, which stays a gate.

- Machine: 13th Gen Intel i9-13900K, 32 logical CPUs, 47 GiB; Ubuntu 22.04
  on WSL2 (kernel 6.18.54.1); Node 22.21.1; Playwright 1.58.0; Chromium 145.
- A (`base1`): PR #75's head as served, worktree `browser-perf-pr75-20261105`
  at `1d82a2b2` (source `5c53870d` plus a docs commit), its release host
  built into its own target directory, its production TodoMVC build.
- B: this branch, `perf/first-press-20261106`, uncommitted then; its own
  release host and production TodoMVC build (`PW_PRODUCTION_ASSETS=1 bash
  spikes/own-renderer/todomvc.sh`):
  - `boot1`: the head's boot preloads, the table asked for beside the
    decision's Wasm;
  - `warm2`: and production warming by module preload (superseded);
  - `warm3`: and warming by the press's own loaders, in need order: the
    ADR's decision.
- Command, each side (the head-to-head's own instrument, TodoMVC only,
  Chromium only, five runs a cell, its rules applied):

  ```sh
  CARGO_TARGET_DIR=<side's target> PORT=13741 node benchmarks/headtohead/head-to-head.mjs \
    --apps todomvc --stacks pleris,react,svelte --engines chromium --runs 5 \
    --conditions cold,warm --no-shaped --no-load --settle 0 --json <side>-loopback.json
  CARGO_TARGET_DIR=<side's target> PORT=13741 node benchmarks/headtohead/head-to-head.mjs \
    --apps todomvc --stacks pleris,react,svelte --engines chromium --runs 5 \
    --conditions cold --no-load --settle 0 --json <side>-slow4g.json
  ```

  `runs/` holds each run's printed report (`.log`) and its raw JSON
  (`.json.gz`). Every run printed `head-to-head: every rule held
  (validate.mjs)`.

## Results (medians; ms unless bytes)

| | A `base1` | B `boot1` | B `warm3` | React | Svelte |
|---|---:|---:|---:|---:|---:|
| interactive, Slow 4G, cold | 3,176 | 1,418 | 1,414 | 1,703 | 1,302 |
| first add, Slow 4G | 2,383 | 2,357 | 96 | 41 | 38 |
| first toggle, Slow 4G | 633 | 614 | 23 | ~30 | ~26 |
| filter, Slow 4G | 29 | 29 | 29 | 17 | 27 |
| first paint, Slow 4G | 1,244 | 1,264 | 1,244 | 1,204 | 1,204 |
| add, loopback, cold | 40 | 42 | 16 | 10 | 9.9 |
| toggle, loopback, cold | 29 | 31 | 15 | 13 | 14 |
| add, loopback, warm visit | 37 | 32 | 6.6 | 7.1-13 | 5.3-8.3 |
| first load received (B) | 52,836 | 53,028 | 175,377 | 70,085 | 20,821 |
| first load requests | 7 | 7 | 19 | 5 | 5 |

React's and Svelte's figures are from the `warm3` runs (each side measured
all three stacks; their medians moved by a few ms between sides).

`warm2` (module preloads) left the first add at 664 ms under Slow 4G in
four of five runs: over HTTP/1.1 the twelve warm requests queue behind six
connections, and the instrument counts a request held over a second as
background, so the press came while the computed module was still
arriving. `warm3` asks in need order and evaluates what it loads.

## The trace

`trace/` holds a Chromium trace of one toggle and one add on `warm3`'s
build (`trace-press.mjs`, `trace-analyze.mjs`, gzipped as ADR-0367 stores
sources), and each one's summary. The toggle's change is made 4.2 ms after
its pointerdown, inside the click's task, and is in the next frame; the
add's 1.1 ms after its keydown. An earlier trace of the preloading version
(`warm2`'s), not retained, showed the press's first `import()` resolving in
a later task, after a frame, and the change 13.6 ms after the press.

## Gates run on the branch

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `bash scripts/case-check.sh`, `python3
  scripts/mutation_anchors.py` (4,992 mutants, each matching once, after
  re-anchoring the three controls the head's new last line moved).
- The server's affected tests (seven, among them the four new ones) and the
  packaging's Node tests.
- In Chromium and Firefox, seventeen spec files the change could reach
  (`lazy-handler`, the new `warm`, `canonical`, `soft-navigation`, `stream`,
  `runtime-size`, `store`, `no-replay`, `page-policy`, `press-modules`,
  `signals`, `computed-modules`, `todomvc`, `device-store`, `navigate`,
  `compiled-handler`, `protocol-boundary`): 252 passed, 16 skipped, none
  failed. Skipped tests are not passes.

## Not run, or failed for the environment

- **WebKit could not open a page on this PC this session**: Playwright's
  WebKit 26.0 launches, and `newPage` then never returns, for a blank page
  with no server too (`/run/user/1000` is absent in this WSL session; a
  private `XDG_RUNTIME_DIR` did not help; user namespaces work). It is the
  environment's, not this branch's: every WebKit test, the unchanged ones
  included, failed the same way. WebKit remains a gate.
- Firefox and WebKit timings, the store, the servers' rows, open tabs and
  idle visitors were not measured in this A/B. The full browser suites and
  exact-head CI were not run.
