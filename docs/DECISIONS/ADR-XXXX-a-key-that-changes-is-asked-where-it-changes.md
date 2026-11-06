# ADR-XXXX: a key that changes is asked for its stale work where it changes

Status: proposed, 2026-11-06, on branch `fix/stale-key-rule-20261106` (a
draft PR against `master`). Decided by the owner on 2026-11-06, choosing
this over rewording PW0325 or requiring the policy of every keyed query.
Not landed; not numbered (PARALLEL.md: the integrator numbers ADRs).
Amends ADR-0152's account of PW0325.

## Context

The charter asks that "changing the query key cancels or supersedes stale
work" (§15.6, test 7). Its corpus of rejections names the class "query key
changes without stale-work cancellation policy". Two rules asked for that
policy, `on_key_change cancel | supersede | keep`:

- **PW0325** (`stale_key_policy`), from E4. It looked at a query alone, and
  refused one that took parameters, declared a `concurrency` and declared
  no `on_key_change`. The `concurrency` clause stood in for "this query has
  a read in flight per key". The guide said so: "A keyed query that sets
  `concurrency` says what happens when its key changes".
- **PW5309** (`signal_key_stale_work`), from ADR-0152. It refuses a page
  whose signal keys a query that declares no `on_key_change`, "whether or
  not it declares a `concurrency`".

Since ADR-0152, a page's signal is the only way a key changes while its
read is in flight:
- A key from a URL never changes within a page: a new address is a new
  document.
- PW5301 lets a signal be read only in a template part, a handler, or a page
  query's key.

So PW0325's stand-in was wrong in both directions:
- **It asked for the policy where it does nothing.** A query keyed only by
  a URL, which declared a `concurrency`, had to declare an `on_key_change`
  that no read ever consults.
- **It let the same query through without one.** A keyed query with
  neither clause passed it.

The owner found the second while reading pleris.dev, where PW0325's
example was a query alone and the rule's trigger could not be explained
from it. Of the repository's 88 keyed queries:
- 33 had neither clause, each keyed only by a URL or the session;
- 1 had `concurrency` alone, a fixture written for PW0325;
- 54 declared the policy.

No program's reader was ever shown an old key's answer because of this:
- where a key can change, PW5309 asks;
- the server applies only a page's latest read (ADR-0152).

## Decision

1. **The policy is asked where the key changes, by one code.** PW5309 is
   the code for "a query whose key changes says what its stale work does".
   The rule behind PW0325 is removed from `rules.rs`.
2. **PW0325 is an old number for PW5309** (`DEPRECATED_ALIASES`). This
   follows the architect's migration rule: where two codes name one
   invariant, keep one, alias the other, and have every detector emit the
   one. It is never reused. Unlike the other old numbers, `pw check`
   printed it.
3. **R-027 states the class as it occurs.** It is now a page whose search
   box's signal keys a query that declares `concurrency` but no
   `on_key_change`: the same query as before, on a page where its key
   changes. It is refused with PW5309, and with the facts its
   `@expect-error` lines name.
4. **The generality fixtures follow the invariant.**
   `examples/generality/stale_key_policy/` becomes
   `examples/generality/signal_key_stale_work/`:
   - `caught.pw` is the shape PW0325 let through: a page's `Int` signal
     keying a query that declares no `concurrency`.
   - `neighbour.pw` is the same page with `on_key_change supersede`, and
     stays clean.
5. **The guide says where the clause is required.** `tour-server.md` now
   says a query a page's signal keys declares `on_key_change` (PW5309).
   `diagnostics.md` is regenerated: 219 codes, and PW0325 among the old
   numbers.
6. **The corpus opens C30** (`docs/CORPUS.md`), since a rejected fixture
   changed. The history keeps what the change did:
   - R-027's text before it, and the removed GENERAL witness, are in
     `examples/history/C30/`, and a test holds that both now check clean;
   - R-027's C8 text is classified `FixtureDidNotExpressIt`, since it
     changes no key, and keeps only the unresolved types C8 repaired.
   One control in `one_checker_mutations.py` removed PW0325's explanation.
   It now removes PW0312's, which R-014 reaches, so the script still holds a
   declaration rule's diagnostic to having one.

## Alternatives

- **Require the policy of every keyed query** (PW0325 without its
  `concurrency` condition). This reads the charter's "for every query key"
  literally. It would add a clause to 33 queries where it does nothing at
  run time, and turn 4 accepted corpus programs into rejected ones.
  Rejected: a rule that asks for a word with no effect teaches nothing.
- **Keep PW0325 and reword it**, saying that `concurrency` is what triggers
  it. Rejected: it leaves the same invariant under two codes, with one of
  them imprecise.

## Consequences

- A program that declares a keyed query with `concurrency` and no
  `on_key_change`, and that no page keys with a signal, now checks clean.
  Before, PW0325 refused it.
  Nothing read its policy:
  - `on_key_change` is consulted only by a page's keyed reads (ADR-0152):
    the server's `read_keyed`, behind `/pw-read`, and the browser's
    `keyedReads`, both for bindings a signal keys;
  - `pw-resource` never reads it.
- The `on_key_change` clauses already written stay valid, and do what they
  did.
- **A future way for a key to change must ask for the policy itself.**
  ADR-0152 did this with PW5309. Examples would be a query read again in
  place for a new address, or a key the browser takes from another
  answer. PW0325 was no net for such a feature: it asked only where a
  `concurrency` was written.
- pleris.dev's `/e/PW0325` becomes an old number's page that links to
  PW5309, once this and the site's PR have both landed.

## Evidence

`docs/evidence/E14/stale-key-2026-11-06/`: the compiler's tests, the guide's
test, the corpus's formatting, the registry's checks, the generated guide's
check, `just preflight`, and every mutation script this change reaches, run
whole.
