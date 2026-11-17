# ADR-0120: three stores, one contract (E14-A)

Status: accepted under the owner's instruction of 2026-10-02 ("start on the
Next.js and SvelteKit stores"). Date: 2026-10-02. Milestone: E14 (charter
§14 M14 task 4, §15).

## Context

E14's benchmark compares the same tasks across React/Next, SvelteKit and
Pleris (charter §14 M14 task 4). A comparison is only fair if the three
starting points are the same store, and that has to be checked by tests, not
asserted in prose.

## Decision

1. **Layout is the charter's (§12):** `benchmarks/baselines/next-react`,
   `benchmarks/baselines/sveltekit`, `benchmarks/harness`. The Pleris store
   stays where it is, `examples/store` served by the own-renderer development
   server. Each baseline is self-contained, because an agent will work in an
   isolated copy of exactly one.
2. **Versions**, verified on the npm registry on 2026-10-02 and pinned exactly:
   Next.js 16.3.8 (16.4.1 since 2026-10-22, 16.4.5 since 2026-11-17, below)
   with React 19.3.0; SvelteKit **2.70.3** (2.70.4 since 2026-11-17) with
   Svelte 5.57.1, Vite 8.3.2 and adapter-node 5.5.7 (5.5.8 since 2026-11-17); TypeScript 6.0.3; Playwright 1.58.0, as
   the own-renderer suite uses.
   - **Not SvelteKit 3.0.0**, released 2026-10-01. A benchmark of coding
     agents on a day-old major measures what the models have not seen, not
     the framework. Revisit when E14-E runs.
   - Next 16 has been released since 2025-10-22. TypeScript 6.0.3 is the
     newest that both SvelteKit 2 and svelte-check accept.
3. **Idiomatic code in each stack, with the same markup contract.** Next
   uses a Server Component page, Server Actions in `<form action>`, and
   `proxy.ts` for the session cookie. SvelteKit uses a `load`, named form
   actions with `use:enhance`, and `hooks.server.ts`. All three render the
   ids, landmarks and text the Pleris store renders (`#store-name`, `#menu`,
   `#cart-count`, `#clear-cart`, the Menu and Cart regions). Data is in
   memory in the server process, as the Pleris development server keeps it.
4. **One behavioural contract, `benchmarks/harness/contract/`**, run against
   built artifacts of all three stores (`next start`, `node build`, the
   compiled Pleris page). A test may know only the store's path and when the
   page is ready. The contract is the store as **all three run it today**:
   the menu, Add, Clear, a cart that survives reload, two quick presses
   making two adds, per-session carts, and a page complete with JavaScript
   disabled.
5. **Negative controls**, `scripts/bench_contract_mutations.py`: a store
   whose count is wrong, whose sessions share a cart, or whose Add adds two
   must fail the contract, each in the stack it is written in.

## Finding: the Pleris store declares what its runtime does not do

`add_to_cart` declares
`optimistic Cart(current_session()) as cart => Carts.with_line(..)` and
`idempotent_by InteractionId`. `pw check` checks both (ADR-0024, ADR-0025,
ADR-0089, ADR-0105). No backend executes either:

- no backend emits code for an `optimistic` clause: the compiled handler
  module, the own-renderer runtime and the Marko adapter contain none, and
  the count moves only when the server's patch arrives;
- the development server's command path takes no interaction id, so a
  retried request adds twice. `runtime/pw-resource` has idempotency keys
  (E4), and the E10 command path does not use them.

So the **shared contract excludes both**, and the Next and SvelteKit stores do
not implement them either. Neither is claimed for any stack. They are what
benchmark tasks T01 (optimistic update with rollback) and T08 (no duplicate
submissions) ask for, and in Pleris those tasks currently need runtime work,
not source changes. Recorded in KNOWN_LIMITATIONS.

## Found while building

The first Next and SvelteKit stores created the session cookie on the first
write. Two presses before any cookie arrived made two sessions, and the
second cookie won: the SvelteKit store counted 1 for two presses. Next
passed only because it runs one Server Action at a time. Both now issue the
session on the first response, as the Pleris server does. This is the class
of defect the benchmark exists to count, and the contract caught it.

## Acceptance

`just e14-contract` (`docs/evidence/E14/contract.txt`): both baselines build
and typecheck, `pw check` passes the Pleris store, the contract passes on all
three stacks three times over, and every mutant is killed.

## Amended, 2026-10-11: svelte-check is asked for the output it is read by

The recipe reads svelte-check's summary in its machine format,
`N ERRORS N WARNINGS`. svelte-check 4.7.6 writes that format by default only
where `CLAUDECODE=1`, the variable an agent's session sets. So the recipe
passed in an agent's session and failed on every verification runner (the
nightlies of 2026-10-09 and -10), where svelte-check writes "svelte-check
found 0 errors and 0 warnings". The recipe now passes `--output machine`.
Next.js 16.3.8, Vite 8 and Marko read the same variable. Marko adds a guide to
its error messages. Next.js and Vite only detect the agent. No other recipe
reads their output.

## Amended, 2026-10-22: Next.js 16.4.1, for three advisories in 16.3.8

On 2026-10-22 the Node audit (`just audit`, charter §3.6) began to fail on
every branch: npm's advisory database now names next 16.3.8 in three
advisories that Next.js's out-of-band release of 2026-10-14 fixed in 16.4.1
and 15.5.28, with no patched 16.3.x
([its notice](https://nextjs.org/blog/nextjs-security-update-october-14-2026)):

- GHSA-673q-mjm6-r5xr, critical: remote code execution, unauthenticated,
  where a `use cache` function returns typed arrays. This store has none.
- GHSA-2464-fhr4-w9xw (CVE-2026-96889), critical: remote code execution
  through the Image Optimization API's SVG path on glibc Linux, in the
  libvips that `sharp` carries. The lockfile's `sharp` was already 0.35.5,
  the notice's workaround, and this store optimizes no image.
- GHSA-p3m5-59mp-jv5r, high: denial of service through a Server Action.
  This store's cart is written by Server Actions.

The baseline pins next 16.4.1, verified on the npm registry, React 19.3.0
unchanged; the owner approved the download. `pnpm-lock.yaml` moves `next`,
`@next/env` and the `@next/swc-*` binaries and nothing else. The store
builds and typechecks on it, `scripts/audit-node.sh` reports no advisory but
the two it documents, and `e14-contract` holds the store to the contract
again. The Next.js findings E14's tasks rest on were read in 16.3.8's source;
16.4.1's passes the same implicit tags to `unstable_cache` (T09). What was
measured before this date, ADR-0342's among it, was measured on 16.3.8.

## Amended, 2026-11-17: SvelteKit 2.70.4, adapter-node 5.5.8 and Next.js 16.4.5, the patches of four accepted advisories

Four advisories the Node audit accepted for these baselines
(`tools/node-audit-allow.txt`, #112, #123, #124) each waited on a patch
release and the owner's approval of its download, given in chat on
2026-11-17. The owner also gave a standing approval: a patch-level security
update to a benchmark baseline is downloaded and landed without asking,
through the same gates (`just audit`, the builds and typechecks,
`e14-contract`, and the pull request's verify).

The versions, verified on the npm registry on 2026-11-17, each the newest
patch of the line this ADR pins:

- **@sveltejs/kit 2.70.4** (2026-11-05, npm's `version-2` tag) fixes
  GHSA-mw6w-2fmv-pvw3 (a `reroute` comparison that crashes adapter-node)
  and GHSA-rqp5-r3wg-vg35 (`router.resolution: 'server'` importing another
  origin's module). Its dependencies are 2.70.3's.
- **@sveltejs/adapter-node 5.5.8** (2026-11-05, `version-2`) fixes
  GHSA-rqw3-x7hx-mm3p (a header added to an immutable request). Its
  dependencies are 5.5.7's.
- **Next.js 16.4.5** (2026-11-03) is the release of Vercel's seven
  advisories of that day: GHSA-mfr8-8qww-v76h, the one the audit reported
  (#124), and GHSA-vc7v-qv4w-wrv8, -vvgp-ww4w-46vf, -rhh4-g77r-r9qj,
  -pc58-hwjq-p385, -7qx7-33qx-rqx2 and -f3mh-rxg3-f5g2, which would each have
  failed the audit once GitHub's database reviewed it. **Not 16.4.6** (2026-11-09, npm's `latest`,
  Dependabot's #23): it fixes no advisory, and it backports bug fixes, among
  them compressing a route handler's response where it was not already
  compressed, a change in what this store's responses are that a comparison
  should choose on purpose, at E14-E with the other baseline versions.
  React stays 19.3.0, within 16.4.5's `^19.0.0`.

`pnpm-lock.yaml` moves `next`, `@next/env` and the `@next/swc-*` binaries,
`@sveltejs/kit` and `@sveltejs/adapter-node`. One move beside them: the
adapter's two rollup plugins resolve `picomatch` 4.0.7, a version the
lockfile already held, where they held 4.0.5. The harness's own lockfiles
(`benchmarks/harness/locks/`, ADR-0124), regenerated from themselves, move
those packages alone: `next-react.pnpm-lock.yaml` from 16.3.8, which the
16.4.1 move had left behind, so a Next sandbox's frozen install failed from
2026-10-22 (NEXT); each now installs offline and frozen as the harness runs
it, once the store holds its packages. Both stores build and typecheck
(Next.js 16.4.5 with Turbopack, `tsc` clean; svelte-check 0 errors, 0
warnings), and `scripts/audit-node.sh` reports no advisory but the four it
still documents: SvelteKit's GHSA-pfhg-q3qv-9q6r and GHSA-27mw-82g8-phjj
(fixed only in 3.x) and GHSA-3r8g-jcqq-3p5j and GHSA-6cf5-gc4q-fm3x (no 2.x
patch). Their reasons hold for 2.70.4 as for 2.70.3. The four fixed
allowances are removed. `e14-contract` holds each store to the contract
again in this pull request's verify. What was measured before this date was
measured on 16.4.1 and 2.70.3.
