# ADR-0172: the cart lists its lines, and a speculation reaches every part that reads it

Status: accepted under the owner's delegation of 2026-10-02. Date: 2026-10-04.
Milestone: E14, the audit's fourth gap (charter §15.3): increment, decrement
and remove, and a cart that shows what it holds.

## Context

Measured on the canonical store on 2026-10-03:

- **The cart was a count.** §15.3 asks for each line to be increased,
  decreased and removed. The data layer had add, clear and current.
- **A speculation set text at the top of a page** (ADR-0122). ADR-0170
  refused a loop, a block or an attribute that reads a speculated value, so
  the cart's lines could not be listed beside its speculated count.
- **An optimistic new line had no name and cost $0.00** (`unpriced()`). A
  transition sees its command's arguments, and `add_to_cart` took an id.
- **The browser cannot address a row it makes.** A row's address is a
  keyed hash under the deployment's key (pw-document), which the browser
  does not hold.

Two races, found by this ruling's browser tests:

- **A speculation could be shown twice.** A command's commit sends the
  cart's new value to the page, and the command's answer says at which
  version. When the value came first, the page held the press and still
  showed it pending, since only the answer could drop it. The count read 3
  where 2 was right, until the answer came. ADR-0122's, since 2026-10-02.
- **Two changes of one session could be sent at once.** A command commits,
  then derives what the session's documents show now and queues it. A
  second command, or a document being served, could derive against the
  same shown document in between. One of the two patch sets then addressed
  a document the other had already changed, and the page lost a line.

## Decision

1. **The store's cart lists its lines.** Each line shows its name, its
   quantity between − and +, its total, and Remove. The cart shows its
   subtotal, and "Your cart is empty." while it is empty. The fees note
   shows while it is not. The section is labelled by its heading, which can
   take focus (`tabindex="-1"`).
2. **A cart line records its item's name and price when it is made**, as
   charter §15.1's `unit_price` does: `CartLine { item_id, name, quantity,
   unit_price }`. `domain.pw` declares `total(line)`, `subtotal(cart)` and
   `fewer(n)`; `unpriced()` is gone.
3. **Three line commands**, each `idempotent_by InteractionId` and
   `optimistic`, each invalidating the cart and emitting `CartChanged`:
   - `increase_in_cart(item)`: one more. The item's availability is read
     again, as `add_to_cart`'s is (ADR-0157).
   - `decrease_in_cart(item)`: one fewer. At one, the line goes; a line that
     is no longer there is not an error, since another page took it first.
   - `remove_from_cart(item)`: the line, gone.

   Each is a step, not `set_quantity(item, n)`. Two presses before the
   first answers are two steps, and a quantity a handler captured would be
   stale by the second press.
4. **`add_to_cart` takes the item as the page showed it**, a `MenuItem`. Its
   transition makes the new line with the item's name and price. The
   command writes by `item.id` alone, and the data layer records the store's
   own name and price, never the request's: a request can send any.
   - **A browser sends a record or a list** of what it can send, each field
     by its Pleris name, each `Int` exact (ADR-0058). Until now a handler
     sent primitives and opaque types over them.
   - **A host reads one by the artifact's parameter types**: a record field
     by field, a list element by element. A missing field is refused by name
     and path, and an extra one is not passed in.
   - **A field crosses by its WIT name**, `minor-units`, and a host finds it
     as `minor_units`. A record with a field whose name would come back as
     another, `opensMinute` as `opens_minute`, is refused at build: it
     could not be found.
5. **A speculation reaches every part that reads its value.** The
   speculation module names each region: an attribute, a block or a loop at
   the top of the page that reads a speculated value. The browser renders
   each one again from the speculated value, with the server's renderer
   compiled to WebAssembly (`pw-render-wasm`):
   - an attribute, as the page writes it;
   - a block, whole, when its rendering changes;
   - a loop's rows, by key. A kept row's parts are set where they are: its
     text, its attributes, and what its handlers capture. Its nodes and its
     focus stay. A row the speculation takes away is detached and kept. A
     new row is inserted at a token the browser makes, until the server's
     row takes its place.

   What a row reads through a member, `line.quantity.count` or
   `line.total.display`, the speculation module computes, as a host does
   for a row (ADR-0169).

   Before a server batch's patches apply, each list is shown as the page
   holds it. After them, the speculation is shown again. A patch always
   finds the document it was derived from.

   A region renders from what the browser holds: the speculated value, the
   page's signals, and the names bound inside it, as a signal's block does
   (ADR-0137). One that reads anything else, or holds a handler that
   captures anything else, is refused. So is a speculated value read inside
   a block no region renders.
6. **Focus.** A row that goes while it has focus gives focus to the same
   control in the next row, else in the previous row, else to what labels
   its region, the cart's heading. Focus in a row is given back after a
   server batch, where the batch replaced the row. WCAG 2.4.3 (Focus Order)
   asks that focus move in an order that keeps meaning and operation, and
   focus on a removed element otherwise falls to the document's body.
7. **The races.**
   - **A value names the presses it includes.** An `entry_value` frame
     carries `applied`: the interactions committed into its value, the
     session's last 64. The page drops their speculations, whether or not
     their answers have come. Replicache's pull response carries the same
     fact (`lastMutationIDChanges`).
   - **A session's changes come one at a time.** A command's commit and
     what it queues, and a document's drain, each hold the session's lock.
     Another session's changes do not wait.

## Alternatives

- **`set_quantity(item, n)`.** Absolute and idempotent, but two quick
  presses both send the same `n`, and the page shows one step for two.
- **A lines query of its own**, as T03's patch adds. The count and the lines
  would be two values of one cart, speculated apart.
- **The server renders the speculation.** A round trip, which a
  speculation exists to avoid.
- **A speculated list replaced whole.** Every press would lose the row's
  nodes, and with them focus.
- **The browser derives the server's tokens.** The deployment's key would
  be public.
- **A speculation dropped by version alone** (ADR-0122's). It needs the
  answer before the value, and the value can come first.
- **One lock for every session.** It would make every visitor wait for
  every other.
- **A command that takes the item's id, name and price.** Three arguments
  to keep in step by hand, for the record the page already shows.

## Acceptance

Recorded by `just e14-cart-lines` in `docs/evidence/E14/cart-lines.txt`:

- `pw-render-wasm`: an instance rendered as the server renders it; what
  changed in one, set in place, or a render where a block changed; an
  attribute's value as the document writes it.
- `pw-render`: what a handler captures is set where it is, once for its
  element; a value a host computed for a row is not captured.
- The compiler (`nested_lists.rs`, `handlers.rs`,
  `every_handler_is_resumable.rs`, `views_compose.rs`):
  - a loop, a block and an attribute that read the speculated cart are
    regions;
  - a region's read, or a handler capture in it, of the store's name is
    refused;
  - a speculated read in the menu's rows is refused;
  - a record and a list are sent as JSON writes them;
  - a variant, a record holding one, and a field named `opensMinute` are
    not sent.
- `pw-conformance`: the four transitions run under Node, with the page's
  count, subtotal and each row's reads over each value, and the held value
  unchanged by any of them. Each command agrees with an independent
  reference.
- `pw-host`: the store's `add_to_cart` reads its item from JSON, and refuses
  a string, a missing field and a fractional price by path; `domain.subtotal`
  reads a list of records and computes from it.
- The development server:
  - each step reaches the data layer;
  - a line records the store's name and price, not a forged one;
  - an item no store has is refused by name;
  - a session's command and its document wait for the change in progress,
    and another session's does not;
  - the value a page is sent names the presses it includes.
- `e2e/cart.spec.mjs`, 7 tests in Chromium, Firefox and WebKit:
  - a line shows before the answer, with its name and price, the count,
    the subtotal, the empty message and the fees, and is the server's
    after;
  - two presses are two steps, and focus stays;
  - one fewer at one takes the line away, with focus to the next line;
  - a refused add takes its line away and says why;
  - Remove sends focus to the heading;
  - a value that includes a press is not shown with it again, before its
    answer comes;
  - focus on a line made before the answer stays on its control after.
- `scripts/cart_lines_mutations.py`: 41 mutants.

**Found by its own test before it was committed:** the speculation module
lowered the page's template without what its handlers capture, so a
region's handler capturing the store's name built. It is lowered with the
capture map (ADR-0134) now.

**Also:** the static build's values (`store-values.json`) set each price's
`display` inside the price, as ADR-0169 did. They set it on the row now, by
its path, as a host does since ADR-0170, so the page's captures carry the
item and not what the page computed from it.

**Found on the way: `pw fmt` wrote `! (a & b)`.** Its rule that a prefix
operator hugs its operand was asked after its rule for a `(`, which spaced
any `(` after a symbol. `with_fewer`'s filter was the corpus's first `!(`.
The prefix rule is asked first now, and `pw-syntax` tests that `!(..)` and
`-(..)` hug and that a binary `n - (..)` does not.

## Not claimed

- **A region inside a block, or a list inside a row**, is not a region. A
  speculated read there is refused.
- **Each Add carries the whole item**, its description included, though the
  command reads its id and the transition its name and price: 714 bytes of
  captures for the three items of the static store. A narrower record is
  the program's to declare.
- **A quantity is not checked at the boundary.** `PositiveInt` arrives as an
  `s64` (the audit's eighth gap).
- **Safari does not focus a button it is clicked on**, as MDN's table of
  click focus records, so the focus tests press from the keyboard.

## 2026-10-28 amendment: a rendering failure owns no unsent speculation

Status: implementation under independent review, 2026-10-29; immutable-head
gates and merge remain pending.

Verify run 40637144982 at `eb28ac01` retained two real WebKit renderer
traps before a like/reply command was sent. Their low-level cause remains
unresolved: the ready waits and module loads completed, and local replay
with the captured renderer has not reproduced them. These failures block
that candidate; no earlier passing execution clears them.

Reading the error path found a separate reproducible defect: `speculate()`
records a pending transition before rendering it. A render exception rejects
before its IDs reach the caller, leaving a transition for an unsent command.
The failed renderer instance also remains cached.

The repair must keep a failure after loading as a failed press (ADR-0382).
It must remove every speculation owned by that press before any fallible
load or redraw, retire only the renderer instance that actually failed, and
attempt to redraw the held state within the existing module-load deadline.
A failed rollback must not retain those pending transitions or replace the
original failure. A later user press may load a healthy renderer. Rendering
failure must never send a command unspeculated, retry a business command,
discard another press's speculation, or claim a commit that did not occur.
Ordinary renderer refusal codes remain refusals, not a reason to replay.

The regression will use the real renderer with selected export bodies
replaced by a valid Wasm `unreachable`, followed by the unchanged renderer.
It must observe no request for the failed press, no ghost reply, and one
optimistic then committed reply for a later healthy press. It must also
exercise a rollback that fails. Existing healthy and request-failure
assertions stay unchanged. The observed WebKit trap's underlying cause is
not claimed repaired by an error-path fix; all fresh exact-commit gates and
independent review remain required before merge.

Review also found that an abandoned renderer load could publish over its
replacement, and that a speculation still awaiting its JS module could adopt
an instance already retired by a signal's native call. Publication must match
both the loading promise and current native owner. An absent or older owner
cannot retire a newer cache. A press resuming after retirement fails before
creating any transition. Known retirement has a distinct platform error type:
it cannot enter ADR-0382's fallback for ordinary import/fetch failures.
The coordinator retains retirement provenance for its own native instance
through either import completion path, including an import rejection or its
existing deadline after retirement. A later healthy coordinator cannot inherit
that older instance's failure. Thirteen lifecycle cases include both import
outcomes through both callers and an ordinary rejection/fallback neighbor.

`renderer-contract.test.mjs` executes selected source fragments from the actual
runtime to control these orderings, alongside real native Wasm instances. Its
held-import cases use Node's experimental main-context VM loader only in tests;
the warning is retained. These seams do not boot a browser or prove a deadline.
The real browser regressions and unchanged module-loading tests cover those
separate boundaries. See the [diagnostic record](../diagnostics/session-renderer-failures-2026-10-29.md).

Local candidate `8da9ab54` then failed Fallow before publication: the loading
success callback, native retirement and test loader introduced three complexity
findings. The successor names the coordinator's coherence predicate, preserves
the synchronous mark/native-clear/speculation-clear phases, and selects test
source fragments in a separate helper. No analyzer policy or domain assertion
changes. The actual failing report and bounded refactor checks are retained
under `docs/evidence/E14/renderer-complexity-2026-10-29/`; fresh successor gates
remain required.
