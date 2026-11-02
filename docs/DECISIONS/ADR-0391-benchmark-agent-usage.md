# ADR-0391: benchmark agent usage is measured separately from pass/fail

Status: accepted, 2026-11-02, with the integrator's rulings below (proposed 2026-11-01). Milestone: E14. Strategy: a minimal adapter contract.

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
unrecognized sidecar refuses the measurement (usage null, the refusal kept
as `usage_problem`); it never becomes evidence, and it never changes the
run's grade.

Exit 75 explicitly means temporary infrastructure failure; shell 126/127,
a command that could not be spawned and a process killed by a signal the
harness did not send also mean failed_at=infra with score=null. A run that
outlives its time budget (`BENCH_AGENT_SECONDS`) is the agent's failure:
failed_at=timeout with score=0. An ordinary nonzero agent exit remains
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
failure, a real timeout and an absent grading stage. `just
e14-agent-telemetry` records the tests and the mutation controls
(`scripts/agent_telemetry_mutations.py`, nine, each killed by an assertion
of its witness test) in `docs/evidence/E14/agent-telemetry.txt`.

## Rulings (the integrator, 2026-11-02)

The review before merging (AGENTS.md rule 4) found two places where the
proposal measured the opposite of what it meant. Corrections first:

1. **A refused sidecar changed the grade.** A malformed sidecar made the
   whole run `ok: false, failure_kind: infra`, so the run went ungraded
   (score null) whatever the agent did. That contradicts this ADR's own
   contract ("an agent's own files and usage claims cannot determine test
   results"). It is also a way out of a zero: the sidecar's path is in the
   agent's environment, so a failing agent, or the model inside it, could
   write one bad byte and leave the comparison. Now the measurement is
   refused (usage null, `usage_problem` kept) and the run is graded as if
   no sidecar had been written. The test `a refused sidecar is a refused
   measurement, never a changed grade` covers both a success and a
   failure.
2. **A timeout was ungraded.** Counting every timed-out run as
   infrastructure would remove an agent's hangs from the comparison. A
   stack whose agents loop until the budget would then look better than one
   whose agents fail fast. Finishing within the budget is part of what E14
   measures, so a timeout scores 0. It is kept apart as `failed_at:
   timeout` so that an analysis can report it. A provider outage is the
   adapter's to say, by exit 75. `sh` now returns the process's signal and
   spawn error, so a timeout (`ETIMEDOUT`) can be told from the other
   statusless ends. The test `an agent that outlives its budget is scored
   0 as a timeout` runs a real one-second budget.

This change does not run any paid model, implement the matrix runner,
enforce token or dollar budgets, isolate untrusted arbitrary shell agents,
or distinguish every build/test infrastructure failure from an application
failure. E14's actual comparative outcome remains unmeasured.
