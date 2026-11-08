# ADR-0334: a command runs when the clock passes its time

Status: accepted under the owner's delegation of 2026-10-02 (the
integrator's; ADR-0332's not-claimed sweep and NEXT 4's third step, orders
scheduled). Date: 2026-10-14. Milestone: E14. Builds on
ADR-0304 (the host's clock, which nothing a program reads but by
comparison), ADR-0325 (an interaction's answer committed with its writes),
ADR-0332 (`invoked_by Payments`, a command the platform runs on a reader's
behalf) and ADR-0280 (`navigate`, a statement that names a declaration and
checks its arguments as a call's).

## Context

- **Two things wait for a time, and nothing runs at one.**
  - ADR-0332 does not claim a sweep: "an order no one pays or takes back
    awaits for good", its cart held and its code used.
  - NEXT 4's third step is orders scheduled: an order placed now, sent to
    its store at the time its reader chose.
  ADR-0304 rejected a command scheduled at each opening and closing for a
  store's hours, which a comparison with the clock decides better. An
  order's deadline is not a value shown: it is something done, once.
- **What the host has** (read 2026-10-14):
  - its one clock (ADR-0304), which `/bench/clock` moves in a test, and
    timers that read a live document again when an instant it compared
    passes, held in memory, by document;
  - an outbox written with a command's writes, which is no queue: a host
    just opened deletes it, and the committing connection delivers and
    deletes its rows;
  - `invoked_by Payments` (ADR-0332): a command no reader presses, run by
    the host on behalf of the reader who began a payment, keyed so it
    commits once (ADR-0325);
  - no program reads the time: `now()` and `now_wall()` answer 0 and no host
    grants `clock.wall`; an instant a program holds comes from its data or
    from `clock.at(date, time, zone)`.
  - a command that writes no rows commits nothing and records no answer.
- **Practice** (each read 2026-10-14):
  - **Convex** ("Scheduled Functions"): `runAfter` and `runAt` from a
    mutation are atomic with it: if the mutation fails, "no function will be
    scheduled"; a scheduled mutation is "executed exactly once", internal
    errors retried, a developer's error failed; `cancel` stops one not yet
    started; each is a row of `_scheduled_functions` (Pending, Success,
    Failed, Canceled); "resilient against unexpected downtime or system
    restarts".
  - **Oban** (v2.24.1, `Oban`): a job inserted "along with other database
    changes, ensuring that everything is committed or rolled back
    atomically"; scheduled "at any time in the future, down to the second";
    unique by its arguments; `cancel_job` leaves a completed job completed;
    retries "scheduled in the future with backoff".
  - **River** ("Transactional enqueueing"): a job enqueued in the
    transaction of the change it follows finds its data committed with it.
  - **Cloudflare Durable Objects** (alarms): one alarm per object, which
    `setAlarm` replaces; "guaranteed at-least-once execution", retried from
    a 2 s backoff, at most six times.

## Decision

1. **`invoked_by Clock`**: a command the platform runs when the clock passes
   the time a command scheduled it for, on behalf of the user (or the
   session) whose command scheduled it. As `invoked_by Payments`: no reader
   presses it (PW6404), it holds no reader to a predicate (PW6401), and it
   is `idempotent_by` its schedule (PW6405). Unlike it: any parameters, as
   its scheduler gives them, and as many such commands as a program needs
   (PW6402 and PW6403 are Payments' alone).
2. **`schedule cmd(args) at when`** and **`schedule cmd(args) after
   duration`**, statements of a command's body:
   - `cmd` is a command `invoked_by Clock`, written as a call (PW6501); its
     arguments are checked as a call's;
   - written in a command, never a page, a view, a handler or a query: what
     a page shows schedules nothing (PW6502);
   - `when` is the clock's `Instant`; `duration` its `Duration`, the time
     from the scheduling command's commit, read from the host's clock then:
     no program reads the time to compute one (PW6503, each type by its
     declaration, so a program's own `Instant` is another);
   - its command performs `clock.schedule` (placement origin), as `emits`
     performs `outbox.write`.
3. **One pending run for each command and its arguments.** Scheduled again,
   it moves; `unschedule cmd(args)` cancels it, and is nothing where none is
   pending. A command retried schedules once, and nothing keeps an id
   (Oban's jobs unique by their arguments; a Durable Object's one alarm).
4. **Written with the scheduling command's writes**, in its transaction: a
   command that answers a declared error, or fails, schedules nothing.
5. **Run once.** The host runs what is due one at a time, in the order of
   its instants, each as a press is run, in a transaction of its own, on
   behalf of its scheduler (`identity::on_behalf`, its principal issued by
   the clock), and keyed by its scheduling (ADR-0325): `schedule-<id>`, the
   same on every host. Once it is answered its scheduling is removed. A
   host stopped after a run's commit and before the removal leaves it
   pending, and the next run of it meets the first's record and is answered
   as it was; two hosts running it commit it once, as two commit an answer
   once. A declared error is the program's answer, and done. A trap or a
   refusal runs again, after 2 s, doubling to a minute, five times at most;
   then the scheduling is kept, failed, never run again, and said in the
   host's log and its `/metrics` (`runs`: `ran`, `retried`, `failed`).
6. **Kept where the program's data is**: each PostgreSQL layer's database
   holds its schedules (a migration of each, `schedules`), written in the
   command's own transaction with its writes and its interaction's record;
   the in-memory layers', the host's materializer, as long as the host
   runs, as it holds their data. A host started reads the earliest pending,
   and is woken by a commit that schedules or by its clock moved.
7. **What the contract says**: a schedule is a call of the platform's
   `pw:host/schedules`, a function by the command's whole path and how,
   `<path>-at` and `<path>-after` given the command's arguments and then the
   time as the `Int` it is, `<path>-cancel` given the arguments; the
   import's `schedules` names the command's component and how, so the host
   reads what it does and never parses a name. The origin grants
   `clock.schedule`.
8. **The first use, ADR-0332's sweep**: `place_order` schedules
   `expire_order(order) after minutes(30)` where it begins a payment; at
   thirty minutes an order still awaiting it is taken back, its payment
   canceled at the provider, as its reader's cancel takes it back (one
   `take_back`). The reader's cancel, and a payment settled for good (paid,
   or canceled), unschedule it; a payment declined keeps it, the order still
   awaiting a card. A sweep takes back only the order it was scheduled for.
   Orders scheduled (NEXT 4's third step) use the same statements, in a
   track of their own.

## Not claimed

- A recurring schedule (cron): neither use needs one.
- A run's precision: it starts once its instant has passed, within a second
  of a host that is running; a host stopped runs what is past due when it
  starts.
- A program reading the time (`now`): `after` is resolved by the host.
- An effect outside the run's transaction asked once: two hosts running one
  run both ask the provider to cancel its payment, as a press sent twice
  does; the provider's cancel is idempotent, and one commit records it.

## Alternatives

- **A schedule id a program keeps** (Convex's `Id<"_scheduled_functions">`):
  a field a program must store, and a retried command schedules twice.
- **The outbox as the queue**: it is delivered and deleted at its commit,
  and a host just opened clears it.
- **`now()` for a program**: every value computed from it is the hazard
  ADR-0304 holds back; a duration the host resolves needs none.
- **A clause, `schedules cmd(args) after d`**: a clause sees only the
  command's parameters, and an order's reference is made in its body.
- **The scheduling removed in the run's own transaction**: the layer's
  commit would carry the run it consumes, for a guarantee the run's record
  already gives (decision 5); a run that writes nothing commits nothing,
  and is run again as harmlessly as it ran.
- **The host parsing an import's name for its command**: a second
  derivation of what the contract can say (decision 7), as `event`,
  `invalidates` and `reads` say theirs.

## Acceptance

`just e14-scheduled`, which needs a database:

- the checker (`compiler/pw-core/tests/scheduled.rs`, 9 tests) and the
  corpus (A-036, R-067 to R-069, three generality pairs);
- the host's runs and the store's sweep (`tests/store_scheduled.rs`), the
  store in memory and on PostgreSQL: the sweep at thirty minutes and not a
  minute before, on its reader's behalf, and told to the reader's open
  order page, though no connection made it; a payment paid cancelling it and
  one declined keeping it; the reader's cancel cancelling it; a press of
  the sweep refused; a sweep of another order taking nothing back; a
  command that answers an error scheduling nothing, and one that writes
  nothing else committing its run; a run at its time, not a second before,
  once, as its scheduler; a run scheduled again moving; a run refused run
  again after 2 s doubling, then kept failed and counted;
- the database's own (`tests/store_pg.rs`, `tests/feed_pg.rs`): a run and
  its command's record written by one transaction; a run outliving the
  host that scheduled it; a run committed and not removed committing once;
  two hosts committing a run once; the feed's layer keeping its runs;
- mutation controls (`scripts/scheduled_mutations.py`, 37): the language's
  12, the host's 20 in memory and 5 on PostgreSQL, each run by the suite
  that must kill it.

## Amendment, 2026-11-07: a run is its scheduler's

Decision 3 kept one pending run "for each command and its arguments", and
decision 1 runs each "on behalf of the user (or the session) whose command
scheduled it". Together they let one reader reach another's run: the same
arguments scheduled by a second reader replaced the first reader's pending
run, which then ran on the second's behalf, and the second's `unschedule`
cancelled it. An independent comparison of two row-ownership designs found
this on 2026-11-07 (ADR-0407's decision 4), and a test reproduced it: the
first reader's espresso went to the second reader's cart.

Decision 3 now reads: **one pending run for each command, its arguments and
the user whose command scheduled it.** That user scheduling it again moves
it, and its `unschedule cmd(args)` cancels it; another user's scheduling is
another run. Oban's jobs are unique by their arguments, and those arguments
say whose job it is; a Pleris command's need not, since the platform knows
its scheduler.

- The key is the user's, the command's and the arguments', each after
  U+001F (`schedules::key`), computed when the command commits.
- Migrations `store/0022` and `feed/0014` give each kept key its user, so a
  run kept under the old key is still its scheduler's, and moves.
- Tests:
  - `another_readers_scheduling_moves_no_run_of_mine`, in memory and on
    PostgreSQL: red with the old key, where only the second reader's run
    is left;
  - `a_runs_key_names_the_user_whose_command_scheduled_it`;
  - `on_postgres_a_run_kept_under_the_old_key_is_its_schedulers_after_the_upgrade`.
- Mutation controls: `scheduled_mutations.py`'s key mutant is re-anchored on
  `schedules::key`, and a new one leaves the user out of it. Both are
  killed.
