# Challenge profiles

Architect ruling, 2026-08-06:

> `29/29` closes the breadth phase. Continue with risk-weighted depth, not a
> universal four-example ratchet.

So each invariant declares a **risk tier** and the transformation dimensions
that are semantically relevant to it — and, as importantly, why the others are
not. Manufacturing four artificial variants to satisfy a uniform number would
produce test code without concentrating effort where a mistake would matter.

## The gates

```text
G1  breadth   29 / 29   CLOSED and frozen
    every registered invariant survived at least one program shape its
    corpus fixture did not anticipate, and has a valid neighbouring program

G2  depth               per-tier, below
```

## Dimensions

```text
aliasing and rebinding      the value is renamed on the way
branching and joins         private on one path, public on the other
helper extraction           the operation moves into a declared function
generic callback            the operation is inside a lambda passed onward
cross-module movement       the declaration that matters is elsewhere
nesting                     the construct is not at the top of the body
deferred execution          a handler, stream, or later frame phase
annotation removal          the author wrote no type
equivalent syntax           the same thing said another way
valid near-neighbour        the same shape, legitimately
```

## Tier 1 — headline and security invariants

Full treatment: direct invalid, indirect invalid, several valid neighbours,
relevant dataflow transformations, cross-module form, execution-context form
where applicable. **8/8 complete**, each with its own `DIMENSIONS.md`.

| invariant | dimensions | notes |
|---|---|---|
| `value_exceeds_sink_level` | 9 | the widest matrix; two neighbours |
| `private_in_shared_cache` | 8 | two neighbours — the invariant is a conjunction; relabelled and reader-keyed since ADR-0128 |
| `declared_placement_cannot_grant` | 5 | two neighbours — a pairing of world and capability |
| `undeclared_effect` | 6 | includes an open row, which is not a claim |
| `forbidden_effect` | 6 | three neighbours — deferred, per-reader, own-world |
| `non_exhaustive_match` | 5 | or-patterns, nesting, wildcard |
| `affine_not_consumed_once` | 6 | branch, match, escape, three neighbours |
| `scope_outlives_owner` | 5 | two neighbours — a comparison of two scopes |

## Tier 2 — compositional semantic invariants

Anything involving value flow, effects, capabilities, privacy, placement,
ownership, control flow, higher-order functions, or cross-module contracts.
**At least three relevant dimensions.**

| invariant | required | have | owed |
|---|---|---|---|
| `secret_to_browser` | rebinding, cross-module, branch join, neighbour | 3 | — |
| `unserializable_capture` | field access, helper return, neighbour | 3 | — |
| `private_in_resume_manifest` | rebinding, branch join, neighbour | 3 | — |
| `unchecked_external_cast` | rebinding, helper return, neighbour | 3 | — |
| `option_used_as_value` | annotation removal, nesting, neighbour | 3 | — |
| `wrong_frame_phase` | helper extraction, nesting, neighbour | 3 | — |
| `observation_feedback_cycle` | helper extraction, neighbour, property choice | 3 | — |
| `false_independence` | nesting, own-definition neighbour | 2 | — |
| `cache_key_omits_partition` | helper extraction, key variation, neighbour (the tenant, since ADR-0128) | 3 | — |
| `task_detached` | deferred execution, nesting, neighbour | 3 | — |
| `no_feasible_placement` | multi-capability, neighbour | 2 | — |
| `dead_internal_link` | arity variation, neighbour | 2 | — |
| `read_per_element` | generic callback with a match arm's key, helper extraction (a function named as the callback), cross-module (another module's read, pair and source), neighbour (a key no element's; a callback applied once) | 3 | — |
| `set_pair_shape` | a reader passed along (a parameter the set form drops), cross-module (another module's set form, of another source), neighbour (the shapes a set form may take: a reader passed along, an answer that holds its key, another module's set form of the same source) | 3 | — |

**Tier 2 complete.** Filling it found three defects, all of the "reads a name
rather than follows a value" family plus one parser bug: `privacy_flow` had
never been migrated to value labels, `annotations.rs` built its type
environment without module context, and a policy value containing a word that
starts a declaration truncated the whole policy block.

## Tier 3 — local structural invariants

A rule over one syntactic shape, where the failure mode is local. **Fixture,
one structurally different challenge, one valid neighbour** — and a targeted
generator only where malformed structure could crash a later phase.

`unkeyed_list`, `invalid_nesting`, `handler_on_inert`,
`control_without_label`, `handler_signature_mismatch`, `unsafe_audit_incomplete`,
`retry_not_idempotent`, `retry_unbounded`, `signal_key_stale_work`,
`optimistic_no_rollback`.

Track `tables` (ADR-0338, D1): `table_malformed`, `data_not_declared`,
`all_unbounded`, `refers_across_sources` and `migration_unstated`, each a rule
over one declaration or one call, its challenge a shape the fixture does not
have (a reference's key, a write by hand, a bare `all()`, a referring table in
no source, a type changed) and its neighbour the same shape, valid.

All complete at 2 files each. `constructor_arity` is tier 3 with a targeted
generator, because malformed structure there **did** crash a later phase.

## Why some dimensions are omitted

- **Branch join** is irrelevant to effect invariants: every branch's effects
  are performed, so there is a union rather than a join and the row must
  already contain it.
- **Annotation removal** is irrelevant where no annotation participates.
- **Deferred execution** is irrelevant to flow rules: a sink call inside a
  handler is still a sink call.
- **Duplicate siblings** is irrelevant everywhere now: resolution is by module
  path since the by-name fallback was deleted.

## Known not covered, anywhere

A label into a collection and back out — `List.push(xs, secret)` then
`List.first(xs)` — is lost, because element types are not inferred. No witness
is written for it, because writing one would require claiming what the fix is.
`labels.rs` records the limit.
