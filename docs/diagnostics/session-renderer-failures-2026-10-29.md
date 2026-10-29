# Renderer failures found while gating the session revocation follow-up

PR62 remains unmerged. Candidate `eb28ac01946367cff453000eed4d39e83beacd9b`
failed verify run 40637144982. The completed actual plan had 153 recipe rows, two
failed: `e14-computed-conditions` and `e14-computed-signals`. The 14 runner
shell jobs succeeded, but the final summary correctly failed. The latter
recipe failed its unmutated browser baseline. Both retained WebKit errors
occurred in the native renderer before the target like/reply request was sent.
Ready waits and module loads had completed. The low-level trap cause remains
unresolved; no low-level cause is claimed established by this repair.
No earlier result clears that candidate or transfers to its successor.

Reading that path found a separate defect: speculation IDs were recorded
before rendering, but a rendering exception prevented those IDs reaching the
caller's rollback. A real `render_part` Wasm trap reproduced a visible failed
press with zero target requests while the page still said "1 reply". The
failed renderer also remained cached for the next user press.

The working repair settles all of the press's exact IDs before awaiting any
redraw, preserves other owners and current authoritative values, retires only
the failed native owner, and preserves the original error if rollback fails.
It reuses a healthy JS module when reacquiring a renderer. Independent review
also required fencing late native-load publication, rejecting adoption of a
renderer retired while JS was loading, and keeping known native retirement
out of the ordinary load-failure fallback. A separate typed platform error
preserves that distinction, with retirement provenance retained by the exact
coordinator through import success or failure. No business command is
automatically replayed.

The initial regression attempt was invalid: it trapped `render_instance`,
while this reply path calls `render_part`; target requests were actually sent.
A subsequent attempt reached the real trap but used the wrong failure-message
role. Both remain uncredited. The corrected cleanup regression failed at the
literal domain assertion (expected 0 replies, actual 1), with a real newly logged
native trap and no request. The second old-runtime case failed the later user
recovery; its rollback-fetch-abort branch only becomes reachable with repair.
A dangling response waiter in that case was moved after the held request was
observed, and the clean diagnostic was rerun before implementation.
The archive's `source-history/renderer-cleanup-valid-red-source-20261029/`
feed copy belongs to the earlier witness with that waiter. It is not the
final cleanup RED's source. That final run's unchanged input maps bind the
feed bytes stored under `working-source/`, including the waiter correction.

Lifecycle tests execute bounded source fragments from the actual runtime.
They use native Wasm instances, controlled fetch completion and an actual JS
import held until after a native signal-render trap. They cover abandoned
load completion, ownership/refusal neighbors, missing-owner retirement,
post-retirement resumption, multi-binding cleanup before awaits, failed
rollback loading, and both the module and press callers' error classification.
An earlier cached-JS ordering proposal did not ensure the trap preceded module
completion; it is uncredited. The ordered import and actual press caller each
failed before their corresponding repair. Import rejection after retirement
then exposed the same fallback in the rejection branch; two new RED assertions
preceded the owner-provenance repair. A healthy native owner's ordinary import
rejection remains a passing fallback neighbor. The thirteen-case green suite retains
Node's experimental VM-loader warning; it is test-only and proves neither a
browser boot nor a module-load deadline.

The real browser tests inject valid `unreachable` bodies into selected exports
of the actual renderer, preserving its other functions, memory and ABI. They
require zero requests for the failed press, its original native failure, no
ghost reply, and exactly one optimistic then persisted reply from a later user
press. A second case aborts rollback's renderer load and still requires the
later healthy press to commit once. Existing healthy/request-failure assertions
are unchanged. The ten original press-module controls remain present; four
anchors are retargeted to the same behavior after the implementation moved.

Reproduce with:

```sh
python3 -m unittest -v scripts/tests/test_renderer_contract.py
just preflight
BUILD_ONLY=1 bash spikes/own-renderer/run.sh
bash spikes/own-renderer/feed.sh
cd spikes/own-renderer
pnpm exec playwright test e2e/feed.spec.mjs e2e/press-modules.spec.mjs --retries=0
```

Run the whole affected mutation gate with `just e14-press-modules` from the
repository root. Working diagnostics and independent reviews are bounded
evidence, with raw records under `docs/evidence/E14/renderer-cleanup-2026-10-29/`.
The final published commit still requires fresh complete CI, security, browser,
recipe and independent evidence review before merge. This record makes no
whole-session completeness or original-trap root-cause claim.

## Completed working verification

The archived `working-broad/receipt.json.gz` records six completed owned
commands: the thirteen-case Node hook, preflight, store and feed builds,
the 93-case browser selection, and the whole press-module campaign. All returned 0;
the captured inputs were unchanged afterward. The browser JSON has 31 passed
cases per engine, with zero skipped, flaky or unexpected results and no retries.
All ten module controls were killed; none was refused or survived, and the
memory guard stopped no process. The new canonical producer output was retained
before restoring the previous tracked canonical file; the restoration receipt
names both hashes. These remain working-tree results, not final-commit gates.

Afterward the Python hook additionally required exact tests/cancelled/skipped/
todo counts; its separate final-green log records the rerun. Runtime and browser
test bytes were unchanged. The final thirteen-case Node fixture was also run
against the preserved earlier runtime as a separately labeled post-repair
negative control: 11 passed and the two rejection-classification assertions
failed. That does not assert those final fixture bytes ran before repair.
