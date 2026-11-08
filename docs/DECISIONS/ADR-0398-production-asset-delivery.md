# ADR-0398: package public browser assets; preserve the private response boundary

Status: accepted 2026-11-07, landed with train-14 under the owner's delegation
of 2026-10-02. Proposed 2026-11-04. This is a measured-loss repair, not a
declaration that Pleris has a production HTTP host.

## Context

ADR-0392 records 237,218 B of first-load transfer and 237,127 B on a warm
TodoMVC visit; neither Pleris's build nor the development host minifies,
compresses or validates public assets. The runtime alone is 157,269 B before
minification. The browser's first interaction also downloads the uncompressed
renderer. Personalized HTML, commands, session and stream responses **must
remain** \`private, no-store\` (ADR-0184); an asset response must carry no
session cookie. No response cache may bypass the resume/build identity check.

## Decision

1. Introduce an **explicit, opt-in packaging step** on the current build:
   minify the shared \`pw-runtime.mjs\` with the already-pinned esbuild version,
   and precompress top-level JS, CSS, SVG and Wasm with deterministic Brotli
   and gzip. Do not rewrite the compiler's component/handler artifacts or
   private HTML. No package is added to the workspace's dependency graph:
   esbuild is already in the lockfile.
2. The development server serves these files with negotiated content
   encoding, representation-specific strong SHA-256 ETags, \`Vary:
   Accept-Encoding\` and \`public, no-cache\` (store only after validation).
   A \`304\` carries no body. Do **not** claim immutable caching for
   unhashed URLs or silently change the source identity.
3. Turn on this production-asset packaging in the head-to-head recipe and
   update its declared expectations. The comparison must still run locally
   on the same machine; old measurements remain historical until rerun.
4. Keep the two-second per-document stream and runtime renderer behavior
   unchanged in this PR. They need their own behavioral/recovery tests and
   end-to-end measurements; HTTP/2 and compiler-produced DOM lowering
   belong in separate, independent architectural slices.

## Verification and acceptance

- An executable packaging test checks minification, Brotli/gzip
  round-trips, byte-identical repeat builds, no private HTML and no
  \`sources.json\` rewriting.
- Native tests check encoding negotiation (including explicit \`q=0\`) and
  strong/weak validator boundaries.
- Run the full exact-head CI, all reached browser/recipe gates, and rerun
  \`just e14-head-to-head\` on the development PC after approval. The
  last measurement is not evidence that this PR improved any observed
  latency, and the host is still the development host.
