# ADR-0391: benchmark agent usage is measured separately from pass/fail

Status: proposed, 2026-11-01. Milestone: E14. Strategy: a minimal adapter contract.

## Problem

E14's offline harness can score replayed reference patches, but a real agent
currently records only elapsed time and stdout. It cannot report token usage
or cost, and an interrupted or unlaunchable agent is scored as though the
model wrote an incorrect patch. This would bias comparisons with Next.js
and SvelteKit.

## Contract

A real agent command receives BENCH_USAGE, a path *outside* its graded
application sandbox. It may write one UTF-8 JSON object with:

- input_tokens and output_tokens, required nonnegative safe integers;
- cache_read_tokens, cache_write_tokens, and turns, optional
  nonnegative safe integers;
- cost_usd, an optional finite nonnegative number;
- model, an optional nonempty model label at most 128 characters.

Input tokens mean **uncached** input, so an adapter that cannot separate
categories must omit the unavailable cache counts and document its own
provider conversion. An absent sidecar records usage as null and no
fabricated cost or token counts. A malformed, oversized, linked or
unrecognized sidecar refuses the measurement; it never becomes evidence.

Exit 75 explicitly means temporary infrastructure failure; shell 126/127,
process execution errors and terminated/timed-out processes also mean
failed_at=infra with score=null. An ordinary nonzero agent exit remains
failed_at=agent with score=0. A successful agent must still
pass the existing checker, build and behavioral graders. A run with no
grading stages is ungraded. The four offline controls continue to score
as before.

This is *adapter-reported telemetry*, not an independently verified billing
record or a security boundary. An agent's own files and usage claims cannot
determine test results. Do not record prompts, secrets or raw API responses
in this sidecar.

## Proof and limits

A no-network stub command must write usage through the real process path;
tests also exercise missing/invalid sidecars, a failing agent, a temporary
failure and an absent grading stage. Run with
node --test benchmarks/harness/test/agent-telemetry.test.mjs.
Hosted verification and mutation coverage are separate gates.

This change does not run any paid model, implement the matrix runner,
enforce token or dollar budgets, isolate untrusted arbitrary shell agents,
or distinguish every build/test infrastructure failure from an application
failure. E14's actual comparative outcome remains unmeasured.
