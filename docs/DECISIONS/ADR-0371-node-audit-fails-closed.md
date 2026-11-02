# ADR-0371: an audit passes only with a complete report from a valid producer

Status: accepted under the owner's delegation, 2026-10-20; assigned by the
integrator to train-12 after the Vite update. Milestone: E14.

## Finding

`scripts/audit-node.sh` discarded the exit status of `pnpm audit`, ignored
its stderr and treated JSON parse errors or absent `advisories` as no
advisories. An empty response, registry error or truncated response could
therefore print `audit-node: OK`. A new regression suite against the
unchanged wrapper ran 22 tests and failed 18 assertions before this repair.

## Decision

Keep the existing documented allowance file and severity coverage. Separate
report validation into `scripts/audit_node.py`, called by the same shell
entry point. A normal invocation captures the exact audit exit status and
stderr remains visible. Only status 0 with no advisories, or status 1 with
advisories, is a valid producer. Status 1 may pass only when every reported
advisory is documented in the existing allowance file. Stdin mode (`-`)
validates a supplied report without claiming an observed producer status.

Before applying any allowance, require a JSON object with no duplicate
keys or error member, an advisory object, metadata with all five known
severity counts as nonnegative integers, and exact agreement between those counts and the advisory entries, by
severity. Every advisory must have its GHSA identity,
known severity, package name and title. A missing allowance file is a
configuration error. Unknown or incomplete shapes fail closed with exit 2;
a valid, unreviewed advisory fails with exit 1. No successful message is
printed for either. A future pnpm report-format change requires an explicit
validator update rather than silently accepting an unrecognized report.

## Count semantics checked against the pinned producer

pnpm 10.15.1's [audit handler](https://github.com/pnpm/pnpm/blob/v10.15.1/lockfile/plugin-commands-audit/src/audit.ts)
reads the registry's severity counts, removes configured ignored advisories
one entry at a time, and increments the corresponding ignored count by one.
Its JSON output retains the original metadata after those removals. Exact
comparison therefore also refuses a report filtered by pnpm's own ignore
configuration; this repository's documented allowance file remains the
single review boundary. The actual reports before and after ADR-0370 agree
exactly by severity: moderate 4/high 3, then high 1. Root review found that
checking only whether both collections were nonempty allowed missing
subsets; two added regression tests failed before the exact comparison. Separate controls weaken the check to nonempty agreement and total-only agreement; each must fail its named missing-subset or severity-redistribution assertion.

## Evidence and limits

Tests exercise the shell entry point with realistic JSON and a separate
fake pnpm process, including producer failures with superficially valid
reports. Mutation controls require named body assertion failures through
the existing canonical Python producer protocol; syntax errors, timeouts
and producer failures earn no kill. `just e14-node-audit` records both.
The regression tests also run in `just evidence-gates`.

This proves validation and failure propagation, not the completeness or
accuracy of the registry's advisory database. Existing documented advisory
allowances are not changed by this repair. Network outages now fail the
audit and require a later successful scan.

The final 24 audit regressions and 26 baseline-harness tests pass locally.
[Before/after command output](../evidence/tooling/node-audit-regression-2026-10-20.txt).
The whole 18-control campaign and full CI remain required on the exact
commit before the train is advanced.


## Landing (2026-10-21)

Landed on `master` with train-12 at `e7664d2eb01aa8c6e91df594bd7270a84a8bf8de`
after exact-commit CI ([39584219053](https://github.com/studiokimeiga/perfect-web/actions/runs/39584219053))
and automatic track verification ([39575516650](https://github.com/studiokimeiga/perfect-web/actions/runs/39575516650)).
Dated statements above that call landing or combined gates pending describe
the candidate before it landed and are kept as written. The
[acceptance record](../diagnostics/train-12/acceptance-record-2026-10-21.md)
states what the landing's evidence does and does not prove.

## 2026-10-29 dependency refresh after a real audit refusal

Both fresh CI events at `88f577a2` refused HIGH
[GHSA-3f7j-7jwc-fxfj](https://github.com/sebhildebrandt/systeminformation/security/advisories/GHSA-3f7j-7jwc-fxfj).
Their Rust audits passed. Cypress 15.21.1 brings `systeminformation` 5.33.15;
the maintainer identifies versions below 5.33.16 as affected and 5.33.16 as
patched. A local normal `scripts/audit-node.sh` invocation also refused before
any dependency change. This candidate remains unaccepted.

Use a root range-scoped override from `systeminformation@<5.33.16` to 5.33.16,
and regenerate the workspace lock with pinned pnpm 10.15.1. The package remains
MIT, with no install lifecycle script. No allowance, severity, validator or
audit policy changes. The SvelteKit sandbox has no Cypress/systeminformation
dependency and needs no unrelated lock change.

The maintainer's [patch](https://github.com/sebhildebrandt/systeminformation/commit/5014c6d25cfce7363b937977327baa853cdba0bf)
changes Windows sanitization and the services-name whitelist. A pure-function
diagnostic on real Windows Node 24.18.0 exercises the installed upstream
sanitizer: curly string delimiters fail before repair while an ordinary
`Spooler` name passes. It calls no service or PowerShell operation and makes
no end-to-end exploit or whitelist coverage claim. Preserve those before/after
bytes and the actual audit refusal; all fresh successor gates remain required.
The [follow-up record](../diagnostics/systeminformation-security-2026-10-29.md)
links the raw evidence and records the actual Cypress compatibility execution.

## 2026-11-02 dependency refresh: glob and minimatch

PR #63's CI at `9ddcfec5` (run 41333137363, "licenses and advisories")
refused two moderate advisories published 2026-10-30; its Rust audit
passed. A local normal `scripts/audit-node.sh` at the same lockfile
refused the same two, exit 1:

- [GHSA-3g3m-2fjr-6pw4](https://github.com/advisories/GHSA-3g3m-2fjr-6pw4)
  (CVE-2026-118773): `glob` `>= 9.0.0, <= 14.0.1`, an `AbortSignal` that
  does not stop an async walk; patched 14.0.2.
- [GHSA-g3hv-75wg-m249](https://github.com/advisories/GHSA-g3hv-75wg-m249)
  (CVE-2026-118764): `minimatch` `< 11.0.1`, a denial of service by
  amplified glob parts; patched 11.0.1.

Neither has a patched release on the line the lock held: `glob` 13 ends at
13.0.6 and `minimatch` 10 at 10.2.6 (the registry, 2026-11-02). Both reach
the workspace only as `spikes/marko-stream-resume` and `spikes/pw-to-marko`
-> `@marko/run@0.11.8` -> `glob@^13.0.6` -> `minimatch@^10.2.2`.

Use a root range-scoped override from `glob@>=9.0.0 <14.0.2` (the advisory's
range) to 14.0.3, and regenerate the workspace lock with pinned pnpm
10.15.1. `minimatch` is not overridden: `glob` 14.0.3 asks for
`^11.0.2`, all of it patched, and a future `minimatch` below 11.0.1 elsewhere
fails this audit rather than being forced across majors unseen. The lock
moves four packages and nothing else: `glob` 13.0.6 to 14.0.3, `minimatch`
10.2.6 to 11.0.4, `path-scurry` 2.0.2 to 3.0.0, and the new
`@isaacs/nonrecursive-glob-match` 1.0.3. All four are BlueOak-1.0.0, as
`glob` 13 and `minimatch` 10 were, by the same publisher, with no install
lifecycle script. No allowance, severity, validator or audit policy
changes; neither benchmark sandbox lock holds either package.

- **A major across `@marko/run`'s range.** `glob` 14 changes walker
  internals, `ignore` patterns that start with `/`, and drops the `debug`
  option (its changelog). `@marko/run` 0.11.8 calls `glob` once:
  `glob("{.tsconfig*,tsconfig*.json}", { root })`, then `.length > 0`. That
  call returns the same matches under 13.0.6 and 14.0.3 in both spikes and
  the SvelteKit baseline, and `marko-run build` of
  `spikes/marko-stream-resume` succeeds with 14.0.3 linked.
- **A release hours old.** `minimatch` 11.0.4 was published on 2026-11-02
  at 04:50Z. Its package is byte-identical to 11.0.3 (2026-10-28), the
  registry's tarball checked against its integrity, but for
  `package.json`'s `version` and `engines` (Node 18 dropped); the upstream
  compare of the two tags shows the same.
- **After:** `scripts/audit-node.sh` prints `audit-node: OK (no new
  advisories; 4 accepted and documented)`, exit 0, and `pnpm install
  --frozen-lockfile --offline` accepts the lock. Revisit when `@marko/run`
  asks for `glob` 14 itself (the override is then removed), or when ADR-0002
  retires Marko.
