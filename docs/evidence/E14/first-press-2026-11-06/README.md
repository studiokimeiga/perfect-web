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

## Later on the branch (`warm4`)

`warm4` measures the branch after three more commits: the renderer's IR
decoder (`pw-render.wasm` 406,192 to 262,527 bytes), a fragment's form
links followed in their press, and compressed compiled modules. Same
command, same machine (`runs/warm4-*.log`, `.json.gz`):

| | `warm3` | `warm4` | React | Svelte |
|---|---:|---:|---:|---:|
| filter, loopback, cold | 31 | **14** | 15 | 30 |
| first add, Slow 4G | 96 | **51** | 35 | 35 |
| first toggle, Slow 4G | 23 | 25 | 24 | 21 |
| interactive, Slow 4G | 1,414 | 1,413 | 1,682 | 1,302 |
| first load received (B) | 175,377 | **140,029** | 70,085 | 20,821 |

A boot profile (`trace/boot-profile.txt`; Chromium's CPU profiler, CPU x4,
five loads of the development build) finds no one cost to cut: the
runtime's evaluation about 11 ms, its activation 16 ms (the decision about
4 ms of it), the device's read 2 ms. What is left of the gap to Svelte under
Slow 4G is bytes on the boot's round trip: the decision's Wasm (21 KB
Brotli) and the runtime (16 KB).

## The store against Next.js and SvelteKit (`base-store`, `fp-store`)

The head-to-head's store app on each side (`runs/*-store.log`): PR #75's head
(`base-store`) and this branch at `096199c2` plus the subscriptions' low
priority, then uncommitted (`fp-store`). Command, each side, after
`PW_PRODUCTION_ASSETS=1 bash spikes/own-renderer/baseline-store.sh` and the
two frameworks' production builds:

```sh
CARGO_TARGET_DIR=<side's target> PORT=13741 node benchmarks/headtohead/head-to-head.mjs \
  --apps store --stacks pleris,next-react,sveltekit --engines chromium --runs 5 \
  --conditions cold,warm --server-runs 3 --settle 0 --json <side>-store.json
```

**Both runs were REFUSED by `validate.mjs`**: three server rounds a stack,
where its rules ask five (`--server-runs 3` was this session's choice, to
save time, and the wrong one). Its nine problems are those counts alone; the
browser cells had their five runs. So nothing below is a measurement of
record; the server rows are three rounds each.

| store, medians | PR #75 | this branch | Next.js | SvelteKit |
|---|---:|---:|---:|---:|
| a press with 19 other tabs open | 10,051 ms | **27 ms** | 40-50 | 47-49 |
| interactive, Slow 4G, cold | 2,540 ms | **1,379 ms** | 2,073-2,079 | 1,754-1,757 |
| add to its frame, Slow 4G, cold | 1,170 ms | **18 ms** | 636-641 | 1,190-1,194 |
| interactive, Slow 4G, warm visit | 2,325 ms | 1,183 ms | 695-700 | 642-658 |
| add, loopback, cold | - | **14 ms** | 30 | 30 |
| server CPU for 20 idle visitors | 18.0 ms/s | 5.00 ms/s | 1.00-2.00 | 2.00 |
| server CPU a request under load | 0.21 ms | 0.28 ms | 6.08-6.35 | 1.38-1.46 |
| peak memory under load | 39.9 MiB | 39.9 MiB | 138-141 | 101-103 |
| first load received | 41,911 B | 45,049 B | 140,212 | 35,026 |

### The store's record, by the instrument's rules (`fp-store-record`)

Then the same at `1cb6cce2` with the default five server rounds a stack:
`head-to-head: every rule held (validate.mjs)` (`runs/fp-store-record.log`).
Pleris is best in 14 rows and loses 10:

| store, medians | Pleris | Next.js | SvelteKit |
|---|---:|---:|---:|
| a press with 19 other tabs open | **23 ms** | 48 | 48 |
| interactive, Slow 4G, cold | **1,381 ms** | 2,083 | 1,753 |
| add to its frame, Slow 4G, cold / warm visit | **18 / 14 ms** | 638 / 615 | 1,189 / 1,172 |
| interactive, loopback, cold | **26 ms** | 76 | 32 |
| add to its frame, loopback, cold | **14 ms** | 30 | 31 |
| the add's bytes | **280 B** | 2,392 | 701 |
| server CPU a request under load | **0.26 ms** | 6.42 | 1.54 |
| peak memory under load | **45.0 MiB** | 142 | 105 |
| interactive, Slow 4G, warm visit | 1,186 ms | 690 | 643 |
| first load received | 45,049 B | 140,217 | 35,018 |
| first paint, loopback, cold | 28 ms | 32 | 24 |
| latency under load, p50 | 1.50 ms | 4.48 | 1.26 |
| server CPU for 20 idle visitors | 6.00 ms/s | 2.00 | 1.00 |
| an idle page's requests a minute | 29.7 | 0 | 0 |

(The idle page's requests are its live subscription, a feature the other
two do not have: their pages hear of no change.)

## Named at one version: a visit after the first (`ver5-*`)

The branch with decision 10 (a production build's files at its delivery id,
kept `immutable`), handler modules not yet versioned. TodoMVC with Slow 4G
also measured warm (`--conditions cold,warm`); the store's record by the
instrument's rules. All three runs: `every rule held`.

| visit after the first, Slow 4G, medians | Pleris | React / Next.js | Svelte / SvelteKit |
|---|---:|---:|---:|
| TodoMVC: interactive | **650 ms** | 1,202 | 1,168 |
| TodoMVC: largest paint | **620 ms** | 1,208 | 1,176 |
| store: interactive (1,186 before) | 640 ms | 686 | 639 |
| store: add to its frame | **15 ms** | 615 | 1,170 |

The store's record (`runs/ver5-store.log`) then loses 8 rows, where the
record before lost 10: first-load bytes and requests against SvelteKit
(45,322 B to 35,018, 7 requests to 6 for Next.js), latency under load (p50
1.61 ms to SvelteKit's 1.33, p99 3.66 to 2.82), and its idle pages'
subscriptions (29.7 requests a minute, 6.00 ms of server CPU a second for 20
visitors, against 0 and 2.00 for pages that hear no changes).

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
