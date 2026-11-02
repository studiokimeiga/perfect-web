# ADR-0390: temporarily fork the benchmark build tools' merge helper

Status: accepted under the owner's dependency-repair delegation, 2026-10-27.
Milestone: E14. Strategy: narrow temporary fork.

PR62's first exact candidate failed both supply-chain jobs on
[GHSA-gcqx-w48m-w2h4](https://github.com/advisories/GHSA-gcqx-w48m-w2h4).
The reviewed advisory describes prototype poisoning of a returned object and
lists no patched release. The npm registry still publishes deepmerge 4.3.1;
the latest pinned Svelte Vite plugin 7.3.1 and Rollup node resolver 16.0.3 both
require it. Their current public extension points cannot replace their imports.
The consumers merge build configuration, rather than Pleris session data.

Vendor the exact MIT release's CommonJS implementation and declarations as
private `@pleris/deepmerge@4.3.1-pw.1`, with upstream license and recorded hashes.
Preserve the merge API, array strategies, custom merge callbacks, cloning and
special-object handling. Reject `__proto__`, `constructor` and `prototype` keys
in both target-copy and source-merge loops, before callbacks or property reads.
Define copied properties as own data properties. This deliberately omits these
three reserved keys; it does not promise general safety for hostile getters,
proxies, cyclic inputs or caller-provided merge callbacks.

Use explicit file overrides in the workspace and isolated Svelte baseline.
The fork lives inside the baseline so the offline benchmark's normal copy
includes it. Regenerate both lockfiles with the pinned pnpm; do not weaken
the advisory validator, allowance file, severities or CI jobs. Test the reported
payload against unchanged upstream before applying the patch, preserve that
failure, then check nested and target-side variants plus valid neighboring
behavior. The ordinary evidence gate also executes the fork's built-in tests.
Actual frozen installs, audit and baseline build/check gates remain required.

Delete this fork and its overrides when both upstream consumers can use a
reviewed fixed release with equivalent regression and offline harness gates.
No claim is made that upstream 4.3.1 itself is patched or the advisory is waived.

Regenerating the isolated lock also exposed its stale Vite 8.3.2 pin despite
the baseline manifest already requiring 8.3.3. Regeneration brings that lock
to the existing manifest, rather than changing the baseline version. Pin
the baseline's packageManager to the same pnpm 10.15.1 as the workspace so
an isolated copy does not select a host's different global Corepack version.
The first unpinned scratch install's environment failure is retained.
Two additional consumer-resolution tests require each installed consumer to
load the byte-exact fixed entry and reject the payload. They run alongside
the 17 API/security regressions in `just evidence-gates`.

## Fallow continuation

Candidate9f's Fallow job found the two directly executed Node test files and
their imported helper unreachable, plus two inherited upstream functions at
CRAP30/42 without supplied coverage. The same five findings reproduce locally
with pinned Fallow3.23.0 and basea0db. Register only the two actual Node test
entry points using the documented `entry` configuration; this adds their real
dependency graph rather than suppressing any file or finding. Keep all ignore,
threshold, policy and workflow values unchanged. Extract the existing per-key
merge selection and options normalization into named cohesive helpers, preserving
guard/value/callback order and same-object option mutation. Identify this local
revision as4.3.1-pw.2. Existing security/API and actual installed-consumer tests,
fresh frozen install/build and an actual passing Fallow audit remain required.
The failed9f run and local reproduction retain their original failure scope.
