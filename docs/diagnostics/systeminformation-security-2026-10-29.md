# A fresh audit refused the vulnerable Cypress dependency

Candidate `88f577a299f26da90ee52654d9debd9b22c45dd6` remains failed and
unmerged. CI push 40918929729 and PR 40918939249 both refused HIGH
GHSA-3f7j-7jwc-fxfj after their Rust audits passed. The original complete
supply-chain logs are retained; successful browser/security results at that
head do not clear the failed Node audits or transfer to a successor.

Cypress 15.21.1 depended on `systeminformation` 5.33.15 through its allowed
`^5.31.1` range. The [maintainer advisory](https://github.com/sebhildebrandt/systeminformation/security/advisories/GHSA-3f7j-7jwc-fxfj)
identifies versions below 5.33.16 as affected and 5.33.16 as patched. Its
[release](https://github.com/sebhildebrandt/systeminformation/releases/tag/v5.33.16)
and [patch](https://github.com/sebhildebrandt/systeminformation/commit/5014c6d25cfce7363b937977327baa853cdba0bf)
cover Windows Unicode quotation sanitization and a services-name whitelist.
Official registry metadata and tarball integrity, package files and MIT license
were independently checked. The release has no install lifecycle script.

The repair adds only the root range-scoped override
`systeminformation@<5.33.16`→`5.33.16` and regenerates the root workspace lock
with pnpm 10.15.1. Package identity/integrity, the Cypress edge and its snapshot
advance together. Cypress, other dependencies, the isolated SvelteKit lock,
audit validator, severity policy and allowance list are unchanged.

Before repair, the normal Node audit failed locally on the same HIGH advisory.
A diagnostic calling the actual installed upstream pure sanitizer on real
Windows Node 24.18.0 failed the Unicode-delimiter assertion and passed an ASCII
`Spooler` neighbor. After repair the same two test bodies passed. It calls no
`services()`, PowerShell, or operating-system mutation: this proves the selected
sanitizer behavior, not exploitability or the separate whitelist. An additional
owned post-repair comparison binds the exact old/new source files, fixture,
Node executable/version, native PIDs and exits; its old-source case is a
post-repair negative control, not a restamped pre-repair run.

The frozen dependency install and actual Node audit then passed, retaining the
four previously documented advisory allowances without adding one. The actual
TodoMVC Cypress compatibility-copy acceptance also ran all 29 tests, including
persistence: zero failed, pending or skipped. Its three owned build/execution
commands returned 0 and the captured working inputs were unchanged. This is
the explicitly identified compatibility copy, not unchanged upstream execution.

Raw source witnesses, commands, reports, native output and scope qualifications
are under `docs/evidence/E14/systeminformation-2026-10-29/`. These working
diagnostics do not satisfy final immutable-head gates. Reproduce with:

```sh
pnpm install --frozen-lockfile
bash scripts/audit-node.sh
bash spikes/own-renderer/todomvc.sh
cargo build --locked -p pw-dev-server
bash spikes/todomvc-cypress/run.sh
```

Require fresh frozen installs, consumer checks, preflight, all planned CI,
security, browser/recipe gates and independent review at the final published
commit before merge. No audit refusal, cancelled run or skipped case earns
passing credit.
