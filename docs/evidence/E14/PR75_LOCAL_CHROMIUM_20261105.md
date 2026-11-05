# PR #75 — local Chromium performance checkpoint (2026-11-05)

Status: **local exploratory verification, not the full E14 head-to-head acceptance**. No remote publication or deployment.

- Source commit: `5c53870dfba2502b14be3a0ffec0aa88d44dcbba` (draft PR #75, stacked on #68).
- Machine: 13th Gen Intel i9-13900K, 32 logical CPUs, 47 GiB, Ubuntu 22.04 WSL2, Node 22.21.1, Chromium 145.0.7632.6, Playwright 1.58.0.
- App: TodoMVC; Pleris release host and production assets compared with pinned React and Svelte production apps. Five rounds per cell, randomized seed 20261025. Benchmark reported `every rule held (validate.mjs)` for both runs.
- Local outputs: `C:\Users\haki\Documents\Codex\2026-11-05\pleris-browser-perf\`.

## Measured network and browser results

| Metric (median where applicable) | Pleris | React | Svelte |
|---|---:|---:|---:|
| Cold first-load received, Chromium loopback | 52,836 B | 70,085 B | 20,821 B |
| Warm first-load received, Chromium loopback | 13,172 B | 1,338 B | 759 B |
| Cold first add received, Chromium loopback | 109,416 B | 0 B | 0 B |
| Cold interactive, Chromium loopback | 39 ms | 44 ms | 25 ms |
| Cold first add, Chromium loopback | 29 ms | 9.0 ms | 7.5 ms |
| Cold interactive, Lighthouse-style Slow 4G + 4x CPU | 3,122 ms | 1,670 ms | 1,298 ms |
| Cold first add, Slow 4G + 4x CPU | 2,333 ms | 32 ms | 30 ms |

The PR's actual runtime (`pw-runtime.mjs`) packages from 157,269 B to 49,880 B minified, 15,674 B Brotli or 17,536 B gzip. A historical 386,022 B renderer Wasm compresses to 91,843 B Brotli (separate existing artifact). The built TodoMVC run instead reported 406,192 B of renderer Wasm in the cold first-add decoded responses, which accounts for most of the 109,416 B delivered on that interaction.

The accepted ADR-0392 historical baseline reports 237,218 B cold, 237,127 B warm, 3,889 ms first add on Slow 4G. Those numbers come from a different source revision and cannot serve as a controlled A/B of this PR.

## Reproduction

1. On the source commit, install workspace dependencies with `pnpm install --offline --frozen-lockfile`.
2. Build `pw-cli`, `pw-dev-server` and `pw-render`; build `pw-dev-server` in release mode; `PW_PRODUCTION_ASSETS=1 bash spikes/own-renderer/todomvc.sh`.
3. Loopback: `PORT=13441 node benchmarks/headtohead/head-to-head.mjs --apps todomvc --stacks pleris,react,svelte --engines chromium --runs 5 --conditions cold,warm --no-shaped --no-load --settle 0 --json /tmp/pw-pr75-todomvc-chromium.json`
4. Shaped: same command with `--conditions cold` and without `--no-shaped`, writing `/tmp/pw-pr75-todomvc-slow4g.json`.

SHA-256 of retained JSON:
- Loopback: `aa27bd9f3101eaa45e05b00b9022a417c0c0767d7619d78faf08111414bd3200`
- Slow 4G: `6ee00f6d0aaf558b9717c4f8b11bfcd050988203be7d201012f91ffd154194f3`

## What remains unproved / next actions

The run excluded Firefox/WebKit, the store app, idle/server-load and twenty-tab measurements, full mutation campaigns and independent review. PR #75 is still a draft; its observed CI/Fallow/failure-census success is not sufficient landing approval.

**Highest-payoff browser direction:** remove the generic `pw-render.wasm` fetch/initialization from ordinary first clicks using a general compiled incremental-DOM update path, with a Wasm fallback only where necessary. Preserve key identity, focus, accessible announcements, safety checks, transactional semantics, and execution/refusal boundaries. Measure download, compile, change-to-frame, 20-tab behavior and cold/warm transfers against React and Svelte. Avoid one-off TodoMVC optimizations or shifting eager Wasm costs into the initial load.

Other measured deficits: private HTML is larger and uncached (`no-store` is necessary), the warm visit still delivers 13.2 KB, and existing per-tab HTTP/1.1 live streams cause substantial contention. Address compression of session-specific responses without relaxing cache privacy; separately consider event-driven notification/transport multiplexing with session isolation.
