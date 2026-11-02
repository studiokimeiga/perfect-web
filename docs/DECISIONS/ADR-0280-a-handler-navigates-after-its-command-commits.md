# ADR-0280: a handler navigates after its command commits

Status: accepted under the owner's delegation of 2026-10-02, on the owner's
brief (relayed 2026-10-08). Date: 2026-10-08. Milestone: E14.

## Context

- **The owner's brief, from a Next.js bug.** After a save, the app soft
  navigates to a page, and the Router Cache serves that page as it was read
  before the save.
- **Next.js** (16.4's guides, read 2026-10-08):
  - A Server Action that calls `redirect` "navigates the router and streams
    the destination's RSC Payload", rendered after the mutation, in one round
    trip.
  - `revalidatePath`, `updateTag` and `refresh` re-render the *current*
    route ([server actions](https://nextjs.org/docs/app/guides/server-actions)).
  - A `router.push` from an event handler, the way to navigate from a
    handler
    ([redirecting](https://nextjs.org/docs/app/guides/redirecting)), goes
    through the client cache. Its `static` stale time keeps a static or
    fully prefetched page for 5 minutes, and "shared layouts won't
    automatically be refetched on every navigation"
    ([staleTimes](https://nextjs.org/docs/app/api-reference/config/next-config-js/staleTimes)).
  - So a page pushed to after a save can be the one read before it. The
    usual fixes are `router.refresh()`, a cache-busting parameter, or a
    hard reload.
- **React Router** (8.4,
  [concurrency](https://reactrouter.com/explanation/concurrency)):
  - After an action, the page's loaders revalidate.
  - A new navigation cancels the pending one's fetches. A new submission
    cancels the earlier one's request, in the browser only: "it can't
    'catch up' and stop it from getting to the server".
  - The later revalidation wins.
- **SvelteKit**: a form action's `redirect(303)`, which `use:enhance`
  follows with `goto(location, { invalidateAll: true })`.
- **Here, until this ruling, a handler could not go to a page.** The
  store's "Place order" said "Your order is placed." and showed a link,
  "Follow your order".
- **Two corrections to the brief as NEXT stated it.** Both were the
  integrator's wording, not the owner's:
  - "a page by name with its arguments checked, as a link is (PW5009)". A
    link names a route by its text, matched segment by segment. PW5009
    checks that match, and a hole's type is never checked (ADR-0275's
    gap). `navigate` checks more than a link does.
  - "its basis carried across the navigation (ADR-0224's rule)". ADR-0224
    rules where a keyed read's value is applied, inside the session's hold,
    and says nothing of navigation. No ADR carries a basis across one.
- **And the runtime has no soft navigation.** A link is a document load:
  `pw-runtime.mjs` has no `pushState`, `popstate` or Navigation API. So the
  Router Cache's failure cannot happen to a link here, and nothing keeps in
  place the parts that two pages share.

## Decision

1. **`navigate Page(args)` is a statement.**
   - It names the page by its declaration, among the views (PW5042
     otherwise).
   - Its arguments are the page's parameters, related as a call's are
     (PW0604, PW0605).
   - Each argument is text, as the page's route gives it (PW0621).
2. **It is written in a handler, last in the `Ok` arm of the command's
   answer nearest it** (PW5043).
   - "Last" reaches through a block's last statement, each branch of an
     `if`, and each arm of a `match` on anything but an answer.
   - An answer bound by a `let` is still an answer.
   - It is refused in an `Err` arm, before the answer, after a command that
     declares no `Result`, followed by anything, or outside a handler.
3. **Its module calls `context.navigate(route, { name: value })`** after
   the command's answer. It is written in the `Ok` case, so it runs there
   only.
4. **The runtime:**
   - **An answer that is not a declared error is a commit.** One that did
     not commit fails the press, every handler's and not only a navigating
     one's.
   - **From the navigation on, the page takes no press and no second
     navigation.**
   - **Each press made before the navigation finishes first**: its command
     answered and its arms run. Only then does the browser load the page's
     address.
   - **The address fills each `{name}` segment of the route with its
     value**, encoded as a link's hole is (`url_component`): every UTF-8
     byte except RFC 3986's unreserved characters. A value no segment can
     carry (empty, `.` or `..`) fails the press.
   - **A navigation that is stopped lets the page take presses again**,
     wherever the browser says it stopped. That is the Navigation API's
     `navigate` signal aborting.
   - **A page shown again from the back-forward cache takes presses
     again.**
5. **The page navigated to is read after the commit, by order alone:**
   - the server answers a command only after it commits and after it sends
     the session's documents their frames;
   - the browser asks for the page only after that answer;
   - the server serves a document after the session's turn, as `no-store`.

   So nothing serves the page from before the save, and nothing busts a
   cache or reloads the page it leaves.
6. **The store's "Place order" goes to the order's page**:
   `Ok(_) => navigate OrderPage()`.

## Acceptance

- **`compiler/pw-core/tests/navigate.rs`, 6 tests**, each with its
  controls:
  - a navigation names a page: not a command, and not nothing;
  - its arguments are the page's parameters, by count and by type;
  - it is written last in the `Ok` arm of a command's answer:
    - refused in a refusal's arm, before the answer, after a command
      answering nothing, followed by anything, and in a refusal's arm
      inside an `Ok`;
    - allowed through a block, an `if`, a nested match, and an answer
      bound by a `let`;
  - one outside a handler is refused;
  - the module goes to the page's route, with its value by name, after
    the answer and only on `Ok`, run under Node;
  - a page with no parameters is given none.
- **`spikes/own-renderer/e2e/navigate.spec.mjs`, 10 tests, in three
  engines but the last:**
  - an order placed goes to its page, served after the commit while the
    command reached the server late:
    - "Placed" as served;
    - `/order` with no parameter;
    - a navigation, not a reload.
  - a refused order stays, and says why;
  - a press made before the navigation is answered first, both ways:
    - its answer last: the page waits for it;
    - its answer first;
  - a press made while the page leaves sends nothing;
  - a second navigation is not taken: the first decides, where without
    the rule each would wait for the other;
  - a navigation stopped takes presses again:
    - where Chromium says it stopped;
    - in Firefox and WebKit, only once the page is shown again;
  - a page shown again from the back-forward cache takes a press, in
    Chromium. Firefox and WebKit do not finish a press made as the page
    unloads, so the stopped navigation's test reaches this there;
  - an `Ok` that did not commit goes nowhere, and the press fails;
  - an address carries each value as one segment, and refuses a value no
    segment can carry.
- **What changed with it:**
  - `pages.spec.mjs` and `accessibility.spec.mjs` follow the order's page;
  - the committed handlers are regenerated (`place_order` is now
    `578d00c93ecefccd`);
  - the store's IR.
- **`scripts/navigate_mutations.py`, 19 of 19 killed** here, recorded by
  `just e14-navigate`. Its first run killed 18: "a page shown again stays
  leaving" survived, because Chromium reached the reset through the
  Navigation API alone. A Chromium test now reaches it through `pageshow`.
  The 23 other scripts this change touches (`ci_plan.py`'s rule, and the
  two specs it changed):
  - three ran whole here before ADR-0281: `command_answers` 16 of 16,
    `press_order` 1 of 1 and `runtime_recovery` 3 of 3;
  - the rest run on CI's verification of `track/navigate`, named at the
    merge.

  The 19 mutants it plants:
  - **the compiler's:**
    - a navigation naming a page;
    - its arguments related, and its page looked for among the views;
    - a refusal's arm, what follows it, a handler's bare body, and an
      answer bound by a `let`;
    - a value given by its parameter's name;
  - **the runtime's:**
    - an answer awaited, not assumed;
    - an `Ok` that did not commit refused;
    - a press while leaving, and a second navigation, not taken;
    - the presses made before it awaited, counted and finished;
    - a stopped navigation, and a page shown again, taking presses again;
    - an address's encoding, and its refusals.

## Not claimed

- **The parts two pages share are not kept in place.** The page navigated
  to is a document load. A soft navigation that keeps them, and that reads
  the page after the commit too, is the next ruling (NEXT).
- **No navigation but a handler's.** A press that goes to a page with no
  command behind it is a link (ADR-0138). Links stay document loads.
- **A link's arguments stay untyped.** `navigate` types its page's
  parameters, but a link still names a route by its text (ADR-0275's gap).
- **A link whose hole is `.` or `..`** is resolved by the URL parser to
  another path. The runtime refuses such a value for `navigate` only.
- **A stopped navigation in Firefox and WebKit.** Neither says it stopped,
  as probed on 2026-10-08:
  - Firefox 146 has no Navigation API;
  - WebKit 26.0 fires its `navigate` event and aborts nothing.

  So the page stays leaving until it is loaded again.
- **A press that never answers holds up the navigation** for as long as
  the browser waits on its request.
- **One host only**: the order of answers and reads is one host's
  (ADR-0246).
- **No navigation on a refusal.** An `Err` arm can show a link instead.

## Alternatives

- **Navigate from the server's answer**, as Next.js's `redirect` in an
  action does. The command would name the page. But a command is one
  request, and one command serves several pages: `add_to_cart` is on the
  store's page and the cart's. The handler knows its page, so the handler
  decides.
- **A soft navigation now.** It needs the document's identity moved
  (its subscription, its signals, its handlers), and history, scroll and
  focus kept. That is its own ruling, next.
- **Take a press made while the page leaves.** It would be sent kept
  alive and committed, but read by no one, and whether the page navigated
  to shows it would race that page's read.
- **Cancel the navigation on a later press**, React Router's "the latest
  wins". The commit is already made. Turning away from its result for a
  press made on a page being left would strand the user on a page that no
  longer says what they did.

## Amended, 2026-10-18: in WebKit, a test lets the page this navigation showed settle before it navigates again

Found by train-10's second verify (39091059482) and ruled by the
integrator on 2026-10-18 (PARALLEL's train-10 entry has the investigation).

- **Corrections first.** The failure was read first as a flake, then as a
  session's turn held across the store page's rebuild, its 400 ms estimate
  included, with the navigation's request waiting behind it. Both readings
  were wrong. With the estimator slowed to 2 s, no session's turn was held
  longer than 63 ms and the navigation's request was answered within 24 ms,
  on train-10 and on master alike. And the 400 ms estimate is master's
  (charter §15.5), not track `scheduled-orders`'.
- **What failed.** Two tests navigate (`goto`) as soon as this ADR's
  navigation has changed the address (the soft navigation, ADR-0306):
  `store-accounts.spec.mjs`' "an address chosen goes back to the store, its
  estimate the chosen one's", after Save, and `accessibility.spec.mjs`' "the
  order's page keeps every rule, as the store moves it along", after Place
  order. Their `goto` cancelled requests the next document's runtime had
  started 1 to 5 ms before (its module, `pw-resume.wasm`, `pw-handlers` or
  its subscription). In WebKit the navigation then failed ("WebKit
  encountered an internal error") or hung to its timeout. That was 4 of
  train-10's 9 WebKit runs on CI (verifies 39054436657 and 39091059482),
  against about 1 or 2 of some 21 runs on other branches.
- **Why.** WebKitGTK's network process corrupts its heap. glibc reports
  `malloc_consolidate(): unaligned fastbin chunk detected` and aborts it.
  Under `glibc.malloc.perturb` it faults instead, in
  `g_type_check_instance_cast` called from libsoup's send path: a
  use-after-free. This is Playwright 1.58.0's WebKit (`webkit-2248`, WPE)
  on the system's libsoup 3 (3.0.7 on Ubuntu 22.04 here, 3.4 on CI's
  24.04).
  - A 60-line script reproduces it against master's own server and program,
    with no spec and none of train-10's code: start a page's requests, then
    navigate a few milliseconds later. 3 of 750 navigations aborted the
    network process.
  - Two things did not reproduce it: a plain Python server serving the same
    paths and sizes (0 of 1,200), and closing a context while a page's
    subscription was open (0 of 1,200).
  - What in this server's traffic trips it is not known (NEXT). Safari on
    macOS uses another network stack.
- **Decided** (the integrator, 2026-10-18). In WebKit, the two tests ask
  for the next document (`nextDocument`) before the press that navigates.
  Once the address has changed, they wait for it to settle before they
  navigate again: its runtime booted (`data-pw-ready`) and its subscription
  answered (a response for a later document's `/stream`). No assertion
  changed. This does not loosen the tests: a person cannot cancel a page's
  requests within milliseconds of their start, and the crash is the
  browser's.
  - Chromium and Firefox still navigate at once. That keeps the case in
    which `accessibility.spec.mjs`' `goto` found ADR-0306's rule that the
    browser's own load wins (Firefox's `NS_BINDING_ABORTED`, ADR-0306's
    finding 2); no mutation control holds that rule.
- **Recorded**:
  - KNOWN_LIMITATIONS, under the browsers' navigation;
  - NEXT, as an open item: what in the server's traffic trips it, a track
    of its own, and whether a newer WebKit build changes it;
  - PARALLEL's train-10 entry;
  - an upstream report, for the owner to file. Nothing is posted from here.

## Amended, 2026-11-02: the settle is one helper, and two more specs wait for it

Found by PR #63's verify (run 41333137334, its WebKit job: 1 failed, 450
passed). Nothing PR #63 changed causes it.

- **What failed.** `store-search.spec.mjs`' "a reader's search lists the
  stores that reach their chosen address" saves an address, whose handler
  goes to the store's page (this ADR's navigation), and navigates
  (`goto /search?q=coffee`) as soon as the address has changed. Its trace
  (the job's `browser-webkit` artifact): the address changed at
  18:18:51.956Z; the store's page asked for `pw-runtime.mjs?document=1` at
  .958 and for `pw-resume.wasm` at .971; the `goto` began at .972,
  cancelled `pw-resume.wasm`, and failed 3.2 s later, "WebKit encountered
  an internal error". The 2026-10-18 amendment's crash, in a test written
  after it (track `store-search`, ADR-0319) with the same pattern.
- **Decided.** The helper the two tests of 2026-10-18 each defined,
  `nextDocument`, is one module, `spikes/own-renderer/e2e/next-document.mjs`,
  unchanged; its two users import it. Three more tests wait for it, in
  WebKit only, before their next `goto`: the failing one, after Save; and
  two of `payments.spec.mjs` that navigated as soon as the same kind of
  navigation had changed the address and had not yet failed: "another order
  waits for the first to be paid", after Place order, and "an order awaiting
  its payment is taken back, its items back in the cart", after Cancel this
  order. No assertion changed; Chromium and Firefox navigate at once, as
  before.
- **Found, not changed.** A sign-up or a sign-in at the development
  provider ends in a whole load of the home page, and some tests navigate
  as soon as its address has changed (`signUp` then `ready` in
  `reviews.spec.mjs` and `store-accounts.spec.mjs`): the same window, after
  a whole load. The helper reads the shown page's manifest, which the
  provider's page has not, so it does not cover them. The open item of
  2026-10-18 (what in the server's traffic trips WebKitGTK) would cover
  every case; until then, a test that navigates right after a navigation
  is the pattern to look for.
