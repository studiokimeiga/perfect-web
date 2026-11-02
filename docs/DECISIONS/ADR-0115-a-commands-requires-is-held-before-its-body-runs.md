# ADR-0115: a command's `requires` is held before its body runs

Status: accepted under the owner's instruction of 2026-10-01 ("Make it
perfect"). Date: 2026-10-01. Milestone: E10 (charter §7.5).

## Context

The charter defines a command as an explicit mutation with an authorization
precondition, and the store has always written:

```pleris
command add_to_cart(item: MenuItemId, quantity: PositiveInt)
    -> Result<Cart, CartError>
    requires SignedIn
{
    Carts.add(current_session(), item, quantity)
}
```

At `87e9786`, `requires` was only policy text. No manifest, component
contract, host, or development server read it. `SignedIn` and
`OwnsOrder(order)` were not declarations in the program, so there was not
even a place that could have supplied their meaning. A request with any session
could run `add_to_cart`.

This is different from a capability. `database.write<Carts>` answers whether
a deployment node may perform an operation at all. `SignedIn` answers whether
this particular invocation by this caller may perform it. Folding the second
into the first would turn per-request authorization into static deployment
authority.

## Decision

**A command's `requires` is a deployment authorization precondition, and every
one is approved before the component body can run.**

1. Predicate names such as `SignedIn` and `OwnsOrder` belong to the
   deployment's authorization vocabulary, not to the Pleris term namespace.
   The compiler therefore does not resolve them as functions or declarations.
2. `requires` belongs to a `command`. Its predicate arguments are command
   parameter names only. `OwnsOrder(order)` is compiled to the index of
   `order` in the command's typed parameter list. Arbitrary expressions are
   not run before authorization.
3. The compiled export carries those requirements in
   `ComponentExport.authorization`. They participate in `abi_schema`, so
   removing a precondition changes the invocation contract even when the Wasm
   value signature is byte-for-byte unchanged.
4. `pw-host` resolves each requirement against the already-typed invocation
   arguments and asks a deployment-supplied evaluator for a decision.
   `false`, an unknown predicate, an evaluator error, or a missing argument
   refuses the invocation before instantiation or any host operation.
5. The raw `call_within` and `call_measured` paths refuse an export that has
   unevaluated requirements. Code that intends to invoke such an export must
   take the explicit `call_authorized_within` path.
6. The own-renderer development server is still not an authentication system.
   It explicitly models every local demo session as `SignedIn`, and defines
   no other predicate. That keeps the demo usable while proving that an unknown
   predicate fails closed. A production deployment must connect the same host
   boundary to its real identity and authorization system.

This makes the compiler responsible for preserving the authored precondition
and the deployment responsible for its real-world meaning. Neither is allowed
to silently reconstruct or omit the other's answer.

## Acceptance

- `compiler/pw-core/tests/authorization.rs`: `requires` accepts deployment
  predicates over command parameters, rejects use on another declaration,
  rejects a non-parameter argument, and rejects duplicated predicates.
- `compiler/pw-core/tests/component_contract.rs`: `SignedIn` survives on the
  exact command export, `OwnsOrder(id)` binds argument 0, and removing
  `requires` changes the ABI hash.
- `runtime/pw-host/tests/contract_mirror.rs`: the committed store contracts
  carry `SignedIn` across the compiler-host serialization boundary.
- `runtime/pw-host/tests/pleris_component.rs`: the raw host call cannot run
  `add_to_cart`; the authorized path can, and no host operation occurs before
  authorization.
- `spikes/own-renderer/server/src/main.rs`: both store commands carry
  `SignedIn`; the development evaluator approves it and refuses an unknown
  predicate without changing cart state.
- `docs/evidence/E8/component-contracts.{json,txt}`: both store command
  contracts record `SignedIn`, and only their ABI hashes move.

## Not claimed

This does not make the development server a production identity provider, and
it does not define application-specific predicates such as `OwnsOrder`.
Those are deployment integrations. The guarantee added here is narrower and
structural: when source declares an authorization precondition, execution
cannot bypass evaluating it.

## Amendment (ADR-0384 H2, track `row-ownership`, 2026-11-02)

The integrator ruled on ADR-0384 (its ruling 2) that a program may give a
predicate a body, so who may act is in the program where the checker sees
it. Built by `ADR-XXXX-owned-tables`:

1. **Decision 1, amended**: a predicate the program declares with a body
   (`predicate Wrote(id: NoteId) says ".." { .. }`) is the programs: its

## Amendment (ADR-0384 H2, track `row-ownership`, 2026-11-02)

The integrator ruled on ADR-0384 (its ruling 2) that a program may give a
predicate a body, so who may act is in the program where the checker sees
it. Built by `ADR-XXXX-owned-tables`:

1. **Decision 1, amended**: a predicate the program declares with a body
   (`predicate Wrote(id: NoteId) says ".." { .. }`) is the program's: its
   body answers a `Bool` (PW0606) and only reads (PW6905), and it is compiled
   as a component of its own, kind `predicate`. A predicate with no body is
   the deployment's vocabulary, as before.
2. **Decision 2, widened**: `requires` belongs to a command or a query
   (ADR-0339's ruling 3); its arguments are still the declaration's
   parameters, so no expression runs before authorization.
3. **Decision 3**: an export's `authorization` entry names the component
   that answers it (`component`) where the program gives the predicate a
   body; it is part of `abi_schema` with the rest.
4. **Decision 4, the order of asking**: the requirements in the order the
   export states them; one naming a component is asked of that compiled
   predicate (in a command's own transaction, or a query's read), and the
   deployment is never asked it; any other is asked of the deployment's
   evaluator as before. False, an error or an unknown predicate refuses the
   invocation before its component runs. The deployment's Rust predicates
   (`identity.rs`'s `holds`) keep working until each is rewritten with a
   body.
