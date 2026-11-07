# ADR-XXXX: a command says who may run it

Status: proposed, 2026-11-06, on branch `feat/mandatory-requires-20261106`
(a draft PR against `master`). Decided by the owner on 2026-11-06: after a
review of pleris.dev found that "a command that requires nothing compiles"
(ADR-0307), the owner chose to make the compiler hold every command to
saying who may run it, rather than soften the site. Not landed; not
numbered (PARALLEL.md: the integrator numbers ADRs).

## Context

- **A command is a request away.** Every executable declaration is a
  component (`contract.rs`), and the server runs `POST /command/<id>` for
  any component it hosts, except one the platform invokes
  (`spikes/own-renderer/server/src/main.rs`, "A command the compiler built,
  by its component id"). A command no page sends is still one request away:
  an address and a body, which anyone can send.
- **What a command could say, and need not.** `requires SignedIn,
  OwnsPost(post)` names predicates the server asks before the body runs
  (ADR-0115, ADR-0302), and PW5048 shows a control only where its command's
  predicates hold (ADR-0307). A command with no `requires` compiled, and
  anyone could run it. ADR-0307 says so: "a command that requires nothing
  needing none".
- **The class.** Broken access control is OWASP's first web risk (2021),
  and the commonest shape of it is an action that forgot its check. A
  framework that lets an action say nothing of who may run it leaves the
  check to memory, file by file.
- **The repository** (2026-11-06): 134 commands in `.pw` files. 69 say
  `requires`, 28 are `invoked_by` the platform, and 37 say neither. A
  further 112 commands are written inside the compiler's tests.

## Decision

1. **PW0367, `who_may_run`.** A command a reader can run, any command not
   `invoked_by` the platform, declares `requires`. A command that says
   neither is refused where it is declared, beside each press that sends it.
2. **`requires Anyone` says "anyone".** `Anyone` is the platform's
   predicate, and every reader holds it:
   - the contract carries no requirement for it, so the server asks
     nothing (`contract.rs`'s `authorization_of`);
   - a page needs no guard for its command's control (PW5048,
     `holds.rs`);
   - it stands alone and takes no argument: beside another predicate,
     or with arguments, it is refused (PW0367);
   - a program that declares `predicate Anyone` is refused (PW0367),
     since nothing would ask its meaning.
3. **`invoked_by` is unchanged.** A command the platform runs declares no
   `requires` (PW6401), and PW0367 does not ask it to.
4. **The migration keeps every program's meaning.** Each of the 37
   commands that said neither now says `requires Anyone`: anyone could run
   it before, and anyone can now, said where it is declared. The commands
   are listed in this change's evidence for the owner to read. The store's
   cart, checkout choices and addresses are among them, by ADR-0318's
   design ("Anyone may fill a cart"). The tests' own programs are
   migrated the same way.

## Alternatives

- **A default**, such as `SignedIn` when nothing is said. Rejected: a
  default is the forgotten decision this rule exists to refuse, made
  silently, and a public command would then need a way to say "anyone"
  anyway.
- **`public command`**. Rejected: Pleris's `public` already says whose data
  a declaration holds (`public query`). Two meanings for one word is a
  defect a reader cannot see.
- **`requires nothing`**. Rejected: it reads as "no check" where the
  program's answer is "checked, and anyone may", and it is not a predicate,
  so every place that reads predicates would special-case it.
- **Only commands a page sends**, as PW0338 asks `idempotent_by` of.
  Rejected: the server runs a command for any request that names it, sent
  by a page or not, so the rule would leave exactly the commands no page
  shows the button of.

## Not decided here (the next step)

- **A command that changes a record a request names by id** (`delete_post(post:
  PostId)`) should require a predicate over that id, or say `Anyone`
  outright. That would close the commonest form of broken access control
  at compile time, and it needs a record to say who owns it.
- **PW0629's predicate with parameters, asked on the page**, so a row's
  Delete shows only where it holds.
- **`pw explain` listing who may run each command.**

## Consequences

- A command that says nothing of who may run it is refused by `pw check`,
  and by `pw build`, which checks first.
- `requires Anyone` is greppable: it lists everything anyone can run.
- Programs written before this change need one line per command. The help
  says which line.

## Evidence

`docs/evidence/E14/mandatory-requires-2026-11-06/`.
