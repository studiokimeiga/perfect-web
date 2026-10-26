# What complex web apps need, and what Pleris does of it

**The north star** (the owner, relayed 2026-10-08): Pleris should be able to
make any complex web app, as perfectly as possible. Twitter, DoorDash and the
apps after them are instruments for that, not the goal. This matrix makes the
claim measurable. Each row is something complex apps need, with its status,
the ADRs that establish it, and what it does not do yet. The next reference
apps are chosen by the rows they cover, not by their brand.

The integrator owns this file. It is updated with each ADR that moves a row,
in the same commit.

**Statuses:**
- **built**: done, with recorded evidence, and its limits stated.
- **partial**: some of the row works, and what does not is stated.
- **missing**: nothing of it yet, or only pieces that do not add up to it.
- **not built here**: charter §2's "Do not initially build" list, or the
  owner's scope.

Checked against train-10 (`track/train-10`, 2026-10-17): `master` at
`32757966` (train-9, landed) and the train's merges, ADR-0001 to ADR-0353
(ADR-0351, generic effects, is train-11's).

**Not yet re-checked after two landings** (note of 2026-10-21). Train-11
landed on 2026-10-20 (`115895de`) and train-12 on 2026-10-21 (`e7664d2e`,
ADR-0351 and ADR-0365 to ADR-0380). The rows below still read as of
train-10, and the [In flight](#in-flight) table is a record of that hour,
not of today's branches. Two examples of how it is now stale: TodoMVC's
official Cypress suite passes all 29 cases on the pinned compatibility copy
with 43 of 43 application fault controls detected, and two hosts share one
database under 50 of 50 controls
([train-12 acceptance](../diagnostics/train-12/acceptance-record-2026-10-21.md)).
Neither is production readiness. A row-by-row reconciliation is pending; until
it is done, [STATUS](../STATUS.md) and the ADRs are the current record.

## Built

| Capability | ADRs | Limits |
|---|---|---|
| Reading, caching and live updates | 0007, 0100–0103, 0107, 0127, 0128, 0145, 0146, 0177, 0219, 0224, 0271, 0294, 0296, 0297, 0320, 0323, 0330 | One host: a second hears nothing of the first's commits (0246). A commit tells up to eight sessions at once; a document no page has asked for in three seconds is passed by and told when its page asks, so a frozen tab sees a change then, not while it is frozen (0297). A block is told part by part only while it shows the arm it showed; another arm, a view's instance or an unkeyed list renders it again whole (0320). A member read is computed in text, attributes, blocks and titles, and through a match arm's binding, not in a loop's list or a view that contains itself (0323, 0330). A list renders in its own length (0294). A document's change is derived outside the host's table, and the trail kept of it is the development server's, for its tests (0296). |
| Commands: optimistic, idempotent, retried | 0025, 0121, 0122, 0154, 0157, 0159, 0173, 0222, 0236, 0238, 0268, 0275, 0305, 0325, 0331, 0346 | A command's arguments are held to their types' invariants before a predicate reads them, from a browser or from a direct component call, inside a map or a set too (0346). An interaction's answer commits with the command's writes, on PostgreSQL and in memory, so a retry after a restart or on another host is answered as the first was; a record answers a day at least, and a declared error records nothing (0325). A page's commands run in the order it sent them, kept by the host that answers (0305). A handler can send an `Option`, a `Result` or a declared sum type; not a case of a type that contains itself, a generic sum type's case whose payload is its parameter, a `Map` or a `Set` (0331). A command declares no timeout (0173). |
| A refusal is never silent | 0302, 0307, 0318 | A predicate has words of its own (PW0351). A refused press shows them beside the control and the page's status region announces them, and every other failed press says so too (0302). A control whose command requires a predicate with no parameters is shown only where the page asks that it holds (PW5048, 0307). A press from a tab whose reader signed in or out elsewhere is refused and told (0318); two windows remain where a retry of a committed press is refused as stale. The words are in one language, the first failing predicate's; a predicate with parameters (`OwnsPost`, `MayMessage`) is not asked of the reader (0302, 0307). |
| Privacy and per-user isolation | 0112, 0128, 0129, 0184, 0252, 0263, 0264, 0270, 0274, 0279, 0282, 0318, 0324, 0341, 0345 | A loop's list and key are checked as every other position is: a read or a secret there is charged its effect and its label (0345). Held by page, `/pw-read`, cache and stream. A label is not followed through storage or time (0129), nor through a materialization's body (0282). A private entry is told to its user's sessions alone where a binding fills its key with `current_user()`; one keyed otherwise reaches every reader of it (0318). `private` says only that a declaration is not importable; `user`, `session` and `public` say whose data it holds (0324). A store's reviews are public without their reviewer: the shared query that shows them is held by the checker to read nothing through a reader's handle (PW5001); a user's id a data layer answered as plain data would not be seen, and the store's answers none (0341). |
| A typed principal | 0263, 0264, 0270, 0318 | The host answers `current_user()`. A guest is a principal of its own; its cart joins the user's at sign-in, and a join that fails leaves the guest's lines reachable by no one (0318). |
| Streaming, out of order | 0148, 0223, 0272 | A region renders once per document, at a page's top only. Without scripts it fills only in Chrome 150 and later. |
| Static pages and SEO | 0114, 0183, 0186, 0189, 0293, 0311 | A page states its title, description, robots, Open Graph and Twitter metadata and, as a page the build checks, its canonical address, which the host writes absolute from the deployment's `PW_ORIGIN` and never from `Host` (0311). kiokun's title and description match kiokun.com's own code on 5,513 sampled words (0293). No alternate-language, icon or manifest link; no JSON-LD (the integrator ruled a typed `Json`, not built); no preview image (an image renderer waits for the owner). |
| Accessibility | 0141, 0143, 0182, 0185, 0302 | Narrow: only the store's pages are audited; no axe-core in the suite; no screen-reader run (KNOWN_LIMITATIONS). A refusal is announced by the page's `role="status"` region (0302). |
| Performance budgets | 0109, 0187, 0188, 0340, 0348 | The long-frame instrument is unstable on the development machine (E7-G8, open). A source read keyed by a loop's element is refused at compile time (PW0405, 0340): scheduled orders' estimate per store, found so when the two tracks met, is one set read now, three statements at both data sizes (0348); no page's or command's bound is computed or emitted, and a write per element is not counted. |
| Uploads (images) | 0260, 0261, 0383 | PNG, JPEG, WebP and GIF. Each upload is rewritten by the host before it is kept: its metadata (Exif and its GPS position, XMP, IPTC, comments, text chunks, timestamps, thumbnails, densities, trailing data) stripped, for every upload; a JPEG's turn kept; colour profiles kept for their colour without their descriptions, device fields or times; animation kept; tested in Chromium, Firefox and WebKit (0383). Not claimed: the pixels, a profile's measured colour tags, data hidden in a JPEG's scan, a gain map (dropped), and images a deployment kept before (not rewritten). A directory as the blob store; one image a post; attaching loses the draft. |
| Pagination | tested under 0222, 0225, 0271, 0296 | Minimal: "Load more" reads again with a larger limit, with no cursor. WebKit's "Load more" failure on CI was the host's lock, and is fixed (0296). The store's search lists the first 20 stores and says how many more match; it does not page (0308). |
| Notifications | 0270, 0274, 0318 | A like, a reply and a follow notify; a mention does not, nothing is grouped, and there is no push and no email. A reader's mark-read reaches their other sessions and sends another user's open page nothing (0318). |
| Direct messages | 0279 | Not end to end encrypted (Missing, below). No blocking yet (NEXT). |
| Keyed reads and cancellation | 0029, 0151, 0152, 0224 | A view's signals and a stream's query are not keyed. |
| Recovery and reload | 0155, 0175, 0176, 0177 | |
| A command outliving its page | 0268 | Within the Fetch standard's 64 KiB kept alive. |
| Resumability | 0110, 0113, 0132, 0134, 0135, 0137, 0139, 0217, 0300 | A build is named by what it built, and the resume decision holds a document to the schema and scope its build says of its page (0300). Across deployments is risk R5, open: a page open across a deployment keeps its old table and document, and that skew is a deployment's (E8). Entries' generation is still the host's constant. |
| Capabilities, WIT components, Wasm | 0006, 0008, 0020, 0026, 0032, 0033, 0116, 0244, 0335, 0262, 0266, 0267, 0283, 0287, 0291, 0317 | A component imports each host operation its compiled code calls, and a handler's work is held where it runs, in the browser (0283, 0287). A call through a local is charged its namesake's effects, which refuses work and grants none (0283). A frame phase's work is still held to a page's declared placement (0287). A component is compiled once a process; a host that loaded many builds would keep each one's, and none reloads a build yet (0317). |
| Routing and not found | 0160, 0162, 0163, 0190, 0231, 0295, 0309, 0346 | A page's parameters are held to their types' invariants as they arrive, and path text that does not decode is a 400, not a missing page (0346). A route's query part (`/search{?q}`) is typed text, `None` when the address leaves it out, and a plain GET form is checked against the route's names (0309). A page can say its address is another address of the page: 308 or 307 to its own route, carrying the route's one parameter and the address's query (0295). A move to another page, or one that adds to the query, is not expressed (0295); a list parameter and a typed number are not read (0309). A program cannot declare its own 404 view. |
| Navigation and layouts | 0160, 0280, 0300, 0303, 0306, 0312 | Links; `navigate` after a command commits, the next page read after the commit (0280). A page names its layout, and a navigation between two pages of one layout and one build keeps the layout in place and shows the next page in it, for a link, a `navigate` and back and forward (0303, 0306). One layout, unnested, given nothing, with one slot and no title template (0303). A page of another layout or build, or with none, is loaded whole; a loop or a view that contains itself is taken whole; no view transition and no prefetch; the document left stays on the host until it is idle, 120 seconds (0306). A stopped navigation is noticed in Chromium only (0280). A link's path arguments are untyped (0280). |
| XSS and CSRF | 0094–0097, 0258, 0333 | Every HTML answer carries a Content-Security-Policy generated from the build and the deployment (scripts and styles by hash, the payment provider framed only where a page frames it), every answer carries `nosniff`, and the build refuses a load from another origin (PW5052) (0333). A program cannot declare another origin; an address a value chooses loads from this origin alone, and nothing at build says so; no `report-to`, so a deployment hears of no refusal; no Trusted Types; one policy for a build's every page. |
| Time and the clock | 0180, 0304, 0328, 0347 | An order can be for a time: fifteen-minute windows from an hour to a week ahead, each one every store of the cart would take an order at by its hours and its estimate, held when chosen and when placed (0347). A value reads the wall clock only by comparing it (`passed`, `today_in`); a kept value holds until the earliest instant it compared, and open pages are told when the clock passes it; zones, dates and weekdays come from the host's tz database. A store's hours are its data and whether it is open is the program's: pages show it, and an order is refused while it is closed (0328). No countdown ("closes in 12 minutes"): no value of now is shown. A handler reads no clock; a page built once and a public materialization cannot compare it; one host's clock, moved forward only; hours look a week ahead; `clock.now()` still answers 0. |
| Scheduled commands | 0325, 0334, 0347 | `invoked_by Clock`, `schedule` and `unschedule`: a command runs when the clock passes its time, once, as the user who scheduled it, its answer committed beside its writes. Runs are kept in the database (or the materializer); a failed run is retried (two seconds, doubling to a minute, five times), then kept, said and counted in `/metrics`. Its first use: an order unpaid for thirty minutes is taken back; its second, an order for a time, paid now and sent to its stores when its preparation is due (0347). No recurring schedule; a run starts within about a second of its time; a program reads no time to compute one; an effect outside a run's transaction, the provider's cancel, may be asked twice by two hosts; one scheduled order a reader at a time, its hours not held again when it is sent (0347). |
| Money | 0169, 0195 (ruling 6), 0327, 0329 | `Money<USD>` is an opaque amount; a part of an amount is taken only at a named rate and rounding (half up, half even, toward zero, away from zero). An order is priced by the program from each store's fee schedule and tax rate, the reader's tip and promo code, and the reader pays the total they were shown or is refused (0329). USD alone; no conversion; `display` is en-US and the store's own code; no discount allocated across stores; a tax rate that changes on a date, or follows a district, is not modelled; each store's tax flags are a deployment's tax adviser's to set. |

## Partial

| Capability | ADRs | What works | What does not |
|---|---|---|---|
| Accounts | 0258, 0263, 0264, 0265, 0270, 0318 | Pleris is the relying party (PKCE, CSRF); a typed principal; the feed and the store both use accounts, and a guest's cart joins the user's at sign-in (0318). | A development provider only, which refuses to run outside a loopback development deployment, and no other provider exists; no OIDC discovery, JWKS or ID-token check; sessions in memory (`track/second-host` is persisting them); no email, recovery, account linking or sign-in rate limits. Google is the owner's choice of first real provider, through a generic OIDC relying party (NEXT, the production path). |
| Forms | 0142, 0143, 0221, 0227, 0265, 0309, 0319, 0331, 0341, 0347 | `bind:value` for a `String`, a textarea, labels, routes checked; a plain GET form checked against its route (0309); a handler sends a sum type (0331). The store saves, renames, removes and chooses delivery addresses, checks out, chooses a time for an order from a page of windows (0347), and rates a store, five star buttons and a comment (0341), with these (0319, 0329). | No checkbox or `<select>` binding; a form as one typed record waits (0131); nothing multi-step. |
| Multi-tenancy | 0107, 0128, 0263, 0264 | A tenant in a cache key; an `Organization` label. | `current_organization()` is bound to `pw:host/organization#read`, which no host answers; no tenant model. |
| Database independence | 0005, 0019, 0207, 0218, 0246, 0298, 0308, 0318, 0325, 0334, 0336, 0337, 0338, 0340, 0348, 0350, 0353 | The feed and the store each run in memory and on PostgreSQL behind one seam (the store's operations are written once over `Rows`); a source states its guarantees and the host holds the layer to them. The store's whole server suite runs on either layer. Search, interaction records, scheduled runs and owners' rows are in both layers. A set read keeps each key's exact result (0348), and a store asked for twice in one gets its hours each time on PostgreSQL as in memory (0353). | Each data layer is written by hand in Rust, about 5,800 lines in `store.rs`, `store_pg.rs`, `feed.rs` and `feed_pg.rs`; PostgreSQL is the only database; one host (0246); the browser suite runs the store in memory. The store's reads are set reads, and each operation's, page's and command's statements are pinned on PostgreSQL, equal at two data sizes (0336). A read keyed by a loop's element is refused by the language, its set read the repair, and a set read on a source that cannot batch is warned of (PW0405, PW0406, 0340); kiokun-site's four such reads are warnings until its layer has set forms. A page's bound is not yet computed or measured: the render merge (`track/render-merge`) is in flight and the conformance suite next (0337). Tables declared in Pleris generate their reads and writes (D1, 0350), and nothing serves one yet (D2). |
| Third-party APIs | 0026, 0148, 0262, 0332, 0387 | `network.fetch` declared and granted; a host binding's shape checked. The host has one outbound client (`pw-outbound`, 0387): only the origins a service's grant declares, every address checked against IANA's special-purpose blocks before any is reached and a name looked up once, TLS 1.3/1.2 (rustls, *ring*) against the platform's roots, no redirect followed, one deadline for the whole exchange, bounded answers, strict HTTP/1.1, secrets written to the connection alone. The payments reach their (development) provider through it, within ten seconds, and check the signature of the provider's webhooks (0332). | No provider at a public origin is configured yet (a Stripe test-mode adapter is NEXT 6d); a program cannot yet name the origins of its own `network.fetch` (ruled: an `origin` declaration carried in its effect row, a track of its own); the recommender is simulated in process; nothing sends a webhook; no post-quantum key exchange, no revocation checking, no proxy; an IPv6-only host cannot reach an IPv4-only provider through NAT64. |
| Payments | 0332, 0334 | A payment flow against a development provider: a command the provider's signed word invokes settles an order (`invoked_by Payments`, HMAC-SHA256, five minutes either way); an order awaits its payment, and is taken back by its reader or by a sweep thirty minutes on; a provider that cannot be reached places nothing; the host holds the key and a program never does. | No real provider; no refunds, held captures or disputes; the reader's return is no second trigger; the provider is asked inside the command's transaction, so an intent begun for a command that then rolls back stays at the provider; `Secret<C>` does not build in a program; `examples/lib/Payments.pw`'s `capture` and `charge` are still `todo` fixtures. |
| Search | 0037, 0041, 0207, 0308, 0309, 0313–0316, 0341 | kiokun: a search page over the whole dictionary (834,036 rows; kiokun.com's two FTS5 statements run in SQLite behind host operations, and the term expansion, grouping, scoring and page are Pleris), the reading index and homophones, and deinflection by rules of our own, each held to kiokun.com's own code where it runs alone (for the search, 351 of 355 sampled queries answer the same, and kiokun.com fails the other four). The store: stores and dishes found by name, cuisine and category in the data layer, on memory and PostgreSQL, to one stated meaning, from a plain GET form, filtered to the stores that reach the reader's chosen address (0308), each with its rating (0341). | No search source an app declares: "a program cannot yet declare a full-text index or its ranking" (0314). The store's search folds no accent or stem, corrects nothing, ranks by no distance, rating or popularity, does not page, and reads every row (`LIKE`, no index). kiokun's `/api/search` has no route form (Pleris has none that answers JSON); Korean is not deinflected. |
| Materializations | 0019, 0103, 0176, 0255, 0273, 0277 | A value its body derives, kept, and made again in order. | Public ones only; made again whole; `regenerate on_read` not run. |
| Structured concurrency and affine resources | 0016, 0045, 0211, 0250, 0251, 0269 | Checked statically (PW20xx). | The `pw-tasks` runtime is not wired into the host. |
| Layout and frame phases | 0187; R-034, R-038, R-040, R-043 | Checked at compile time. | No frame scheduler in the runtime: no `requestAnimationFrame` or observers. |
| Developer experience | 0013, 0149, 0276, 0321, 0322, 0343, 0345 | The compiler's feedback: `pw check` (coded diagnostics with spans, and notes shown in the file they belong to), `pw explain` (types, effects, privacy, placement), `pw fmt` (one canonical layout), `pw diff` (what a change means), `pw build`. A guide and a tutorial, `docs/guide/`, whose every example the real `pw` checks, its tutorial the guestbook a browser test drives, and the codes' reference generated from the registry (0343). `&&` and `||` are refused with the hint that Pleris writes `&` and `|` (PW0009, 0322), and an effect in a loop's head is refused as anywhere else, which the guide now shows (0345). | The guide covers what a newcomer needs first, not the language (no streams, materializations, uploads, payments, resources or WIT components yet), and `docs/SEMANTICS.md` still reads as the M0-era status; `pw check` passes programs `pw build` refuses, a pure computed list among them (the guide holds them as examples; `track/check-is-build`, train-11); no editor support or language server ("none exists yet", 0321; the charter asks for one); no `pw new`; no live reload (a host serves one build, 0317); a small standard library (no `split`, no `range`); no debugging aid beyond `PW_TRACE` lines, `GET /metrics` and the development server's per-document trail (0296). |

## Missing

| Capability | What exists | Covered by |
|---|---|---|
| Admin tables: sort, filter, bulk edit | `List.sort_by` and `filter` (0040); a table's nesting checked (0204). No sortable table, no bulk command, no checkbox binding. | The SaaS admin app. |
| Email, outgoing webhooks and recurring jobs | A `task` declaration kind is parsed and does nothing (its signature skipped, Koka refusing it); `task.spawn` is checked statically (PW2001–PW2003); `pw-tasks` (0016) has Rust tests only, no timers, and the host does not use it. A command can be scheduled to run once (Built, 0334); there is no recurring schedule, no email and no outgoing webhook. The host hears one kind of webhook, the payment provider's, signed (0332). | The SaaS admin app. |
| Charts and dashboards | None. | The SaaS admin app. |
| i18n and l10n: locales, currencies, plurals, RTL | A locale as a cache key's dimension only (0056, 0169, 0180, 0186, 0255). A program has one set of words for a refusal (0302); `display` writes US dollars as en-US does (0169). | The SaaS admin app. |
| Real-time collaboration and offline | `replicated` is parsed only (A-011); charter §9.1's kind 4. Its design is ruled (0339): Automerge behind `replicated`'s `rich_text` policy, sync and presence on the stream. Nothing is built, and its downloads wait for the owner. | The Google Docs-shaped app. |
| Maps, canvas and custom widgets | A mounted resource is refused by the build (0075). | The Google Docs-shaped app's editor, ProseMirror mounted at the page's authority (ruled, 0339). |
| End-to-end encryption | None: a device-only placement label, browser-placed computation and rendering, and device-local storage are each missing. | The end-to-end-encrypted messages after the Docs app. |
| Video and media | Uploads are images only (0260). | No planned app yet. |
| Experiments, feature flags, analytics events | `Analytics.pw` and `Telemetry.pw` are `todo` stubs. | No planned app yet. |
| AI features: a model's tokens streamed into a region | A region renders once per document (0148). | No planned app yet. |
| A second host, horizontal scale | A second host on one database hears nothing of the first's commits (0246). A session and a sign-in under way are in one host's memory (0258), and a page's command order is its host's (0305). An interaction's answer is committed with the writes (0325), and a scheduled run is keyed the same on every host (0334). | `track/second-host` (in flight). |
| Production | Pieces, not a deployment: `PW_DEPLOYMENT` and `PW_ORIGIN`, a generated Content-Security-Policy, SQL migrations applied at start, `GET /metrics`. The host is the development server (notes below). | kiokun.com, the owner's production target (ADR-0285): served first on a preview origin, with no cutover without the owner's go (NEXT). |
| Interop: JavaScript components and npm libraries in a page; incremental adoption into an existing site | Nothing loads JavaScript into a page, and nothing adopts Pleris into an existing site (notes below). | The Google Docs-shaped app's mounted editor (ruled, 0339) is the first use of a resource. |
| Testing tools for app authors | `pw check` and `pw fmt --check`; no `test` declaration, no `pw test`, no fixtures or browser harness for an author (notes below). | No planned app yet. |
| A data layer declared in Pleris, not hand-written Rust per app | A `source` declares guarantees only (0207); the layer behind it is about 5,800 lines of Rust for the feed and the store. A `table` declaration generates its reads and writes, its schema's metadata and a migration's lock, its generated pairs held by the bounded checker (D1, 0350); no adapter serves a table yet (notes below). | Ruled (0338): one generic adapter per database; D2, the memory adapter, with TodoMVC (`track/tables-d2`, in flight). |

### The new rows, in detail

**Production**

- `PW_DEPLOYMENT=production` and `PW_ORIGIN` exist. A production deployment
  whose pages state a canonical address, and states no origin, is refused at
  start (0311).
- A Content-Security-Policy is generated from the build, and every answer
  carries `nosniff` (0333).
- SQL migrations, each layer's own (store 0001 to 0015, feed 0001 to 0009),
  are applied in order at start under an advisory lock, and recorded.
- `GET /metrics` answers how often each data-layer operation ran, how many
  values are kept and how scheduled runs ended (0127, 0334). A webhook's
  signature is checked (0332). A command's body is bounded to 64 KiB.
- The host is the development server, "NOT E8's production host" in its own
  Cargo.toml. There is no container image or deployment target.
- The host serves plain HTTP, a thread for each connection; outbound, it
  speaks TLS through its one client (0387).
  It compresses nothing, and sends the runtime as written, unminified (0188).
  There are no causal traces (charter §10.5); logs are lines on stderr. No
  rate limit exists but an upload budget (0260).
- Sessions live in one host's memory (`track/second-host` is moving them),
  and the identity providers are development ones (0258); Google's is queued
  in NEXT.
- Secrets: the payment provider's key is the host's and no program holds one
  (0332), `Secret<C>` does not build in a program, and the development
  provider makes its webhook secret at start. Nothing manages a deployment's
  secrets.
- Queued in NEXT and not built: migrations across live deploys, secrets
  management, rate limiting and backups. Wasmtime's advisories of
  2026-10-14 are fixed by 48.0.6 (0335), and those of 2026-10-19 by 48.0.7
  (0363).

**Interop**

- A page writes no script (0094), the generated policy allows scripts from
  this origin only (0333), a program cannot declare another origin, and a
  build has no assets. So a JavaScript library cannot be loaded into a page.
- A mounted resource, the charter's boundary for imperative widgets, is
  refused by the build (0075). A custom element can be written in lowercase
  (0072), and nothing loads its code.
- Handlers compile to JavaScript the runtime loads: Pleris out, not
  JavaScript in.
- For adoption into an existing site: every page is a whole document the
  host serves, and no ADR or design names an embedded or partial mode.

**Testing tools for app authors**

- `pw check` and `pw fmt --check` run on an app's program.
- The reference apps' test controls are their own: `/bench/clock` moves the
  host's clock forward (0304), `/bench/records` answers a document's trail
  (0296), and `store::Faults` injects delays and failures into the store.
- The language has no `test` declaration and the CLI no `pw test`. There are
  no fixtures or fake-data helper, no browser harness shipped to authors, and
  no coverage.
- The guide compiles its examples with a test and drives its tutorial's
  app in browsers (0343), and `track/todomvc` (in flight) holds TodoMVC to
  the upstream Cypress suite, which it does not pass yet (3 of 29). Each
  tests Pleris or one app, not a tool an author runs.

**A data layer declared in Pleris**

- A `source` declares guarantees only (0207): `FeedData`, `StoreData`,
  `KiokunEntries`, `KiokunSearch`.
- The layer behind it is Rust in the host: about 5,800 lines in `store.rs`,
  `store_pg.rs`, `feed.rs` and `feed_pg.rs`, written once for memory and once
  for PostgreSQL, with SQL files for the schema and the seed data inside
  them.
- Replacing `store:data/carts` with compiled Pleris is E10-S10, open in the
  evidence ledger since E10.
- ADR-0338 (ruled 2026-10-15) declares tables, keys, indexes and references
  in Pleris, one generic adapter per database, and migrations generated from
  the declarations. D1 is built (0350, train-10): the `table` declaration,
  its checks (PW0361, PW0362, PW0364 to PW0366), its generated functions,
  `tables.json` and a schema's diff; its reads are deterministic, and its
  set reads' pairs are generated for the bounded checker. Nothing runs them: D2, the memory
  adapter with TodoMVC, is in flight (`track/tables-d2`, uncommitted).

## Not built here

**Charter §2, quoted:** "Do **not** initially build any of the following: a
database engine; a SQL optimizer; a new TCP or QUIC implementation; a
complete browser engine from scratch; a CSS layout engine; a global CDN; a
distributed database; a package registry; a bespoke binary replacement for
HTML; a custom router appliance; a local foundation model; a universal CRDT
system; a full production framework before proving the semantic model."

- "A custom router appliance" is network hardware, not URL routing or
  dispatch.
- `docs/vision/non-goals.md` heads this list "Never build", which is
  stronger than the charter's "initially". That file says a change to it
  needs an ADR and a human decision, so it is recorded here and not edited.

**The owner's scope for the DoorDash app** (relayed 2026-10-08): courier
dispatch, routing and fraud are the backend's services, outside the app
layer. Payment processing is a provider's: Pleris holds a capability, and
never card data.

**Reused, not built:** a search engine (the database's, or a declared search
source), and a CRDT or OT library behind `replicated`.

## In flight

Branches being built now (2026-10-17, about 13:30 UTC, as each track's
progress file and commits show it). None is on `master` or on train-10,
which lands evidence integrity (0344, 0349), each-head effects (0345),
argument invariants (0346), scheduled orders and set reads that keep each
key's result (0347, 0348), set pairs held to their key by name, D1 tables
(0350), a repeated store's hours (0353) and the planner's ceiling (0290
amended); they go on train-11. None of them is pushed. A row moves when its
branch merges with its ADR and its evidence, and not before; what a line
says here is the branch's aim, and its state as its commits and progress
file show it at that hour.

| Branch | What it builds, and where it stands | Rows it moves |
|---|---|---|
| `wip/train10-generic-merge` (from `track/generic-effects`) | Effects and labels carried through generic and member calls (ADR-0351, 0345's not-claimed). Merged onto train-10's base; of its 49 related scripts 30 green, lexical 18 of 19, the rest running; no `just ci` yet. | Privacy; developer experience. |
| `track/kiokun-cache` | Kiokun's search cache published safely under concurrent builds, with each reader's version kept alive (NEXT's integration follow-up). Its own 10 controls, the word script's 138 and its `just ci` green. | Search (kiokun). |
| `track/check-is-build` | `pw check` refuses whatever `pw build` refuses, within the check's 2 s bound. Merge-ready on its own base; its assembly onto train-10 is green and uncommitted, held for review. | Developer experience. |
| `track/tables-d2` | D2: one generic memory adapter for declared tables (0338). Its validated work is uncommitted (whole 70 of 70); no `just ci`, no TodoMVC acceptance yet; a provider-cost proposal waits for a ruling. | A data layer declared in Pleris; database independence. |
| `track/todomvc` | TodoMVC held to the unchanged official Cypress suite: 3 of its 29 tests pass, 23 do not run, and the corrected gate fails it. Needs D2, the browser's local persistence and a timing fix. | Routing; forms; testing tools; a data layer. |
| `track/render-merge` | One render's identical reads made once, from one snapshot. Master merged in; the payments pins held in 12 of 12 runs; the bounded render pool, its whole mutation script and `just ci` left. | Reading and live updates; database independence. |
| `track/second-host` | Hosts sharing one PostgreSQL database: an outbox and a listener, sessions and sign-ins persisted. 19 two-host tests pass; its rulings, end-to-end tests, mutation script and `just ci` left. | A second host; accounts; reading and live updates. |
| `track/contributor-dx` | A contributor's workflow: quickstart, change-scoped checks, worktree recovery, slimmer documents. Its quick checks pass 307 of 309; CI on the combined source and a second machine left. | Developer experience. |
| `track/browser-host-map`, `track/browser-cleanup-23`, `track/guide-producer-validity`, `track/parallel-instrument` | The evidence's own: one registry of browser hosts, the 23 browser mutation scripts counting only real assertions, the guide's harness counting only real failures, and mutants run in parallel slots. Each green on its own; the first two uncommitted, held for review. | The evidence, which is not a row. |

## The reference apps, by the rows they cover

In the owner's order (NEXT), with where each stands:

1. **The DoorDash customer side**: built, with train-10.
   - Built: client-side navigation (0306), accounts in an app (0318),
     database independence (the store on PostgreSQL, 0298), search (0308),
     time and the clock (store hours, 0304, 0328), money (0327, 0329),
     payments against a development provider (0332), forms (addresses,
     0319, and checkout), a command that runs at a time (an unpaid order
     taken back, 0334), its reads by set, their statements pinned on
     PostgreSQL (0336) and a read per element refused (0340), and ratings
     and reviews of a delivered order's stores, public and unmoderated
     (0341), and orders for a time its reader chooses, sent to the stores
     when their preparation is due (0347).
   - Its places are a fixture of eleven, not a geocoder, and its courier
     minutes a model (0319).
2. **The bug museum, extended, and the head-to-head comparison**: the
   evidence that answers "is Pleris perfect?", not breadth. The museum is
   begun (0342): five exhibits, each a census leaf with a dated primary
   source, its bug shown happening in Next.js or SvelteKit, and Pleris's
   answer. F01, G01 and J08 are refused by a code, J01 at runtime; H07, an
   N+1 read, was a miss at the museum's first record and is refused since by
   PW0405 (0340), the museum's first closed miss. The head-to-head's
   protocol is written, TodoMVC first; nothing of it is measured, and its
   downloads wait for the owner. Next: J01's variant on a streamed page.
3. **A Google Docs-shaped app**:
   - real-time collaboration and offline;
   - presence;
   - a mounted editor surface;
   - sharing and permissions;
   - version history.

   Its design is ruled (0339): Automerge, a mounted ProseMirror, sync and
   presence on the stream, its data declared as tables (0338). Nothing is
   built, and its downloads wait for the owner.
4. **End-to-end-encrypted messages**:
   - a device-only placement;
   - browser-placed computation;
   - device-local storage.
5. **A multi-tenant SaaS admin**:
   - multi-tenancy and roles;
   - admin tables;
   - charts;
   - i18n;
   - background jobs and email;
   - webhooks;
   - an audit log.
6. **The app on three kinds of database**: database independence, past
   SQL, and a data layer declared in Pleris. The owner approved its
   databases (NEXT): SQLite, libSQL or D1; MongoDB or Firestore; Cassandra
   or ScyllaDB; and, optionally, CockroachDB. Its design is ruled (0338):
   the data declared in Pleris, one adapter per database. D1, the tables,
   is built (0350); D2, the memory adapter with TodoMVC, is in flight; the
   owner's decisions 1 to 7 come before the MongoDB, Cassandra and
   CockroachDB tracks.

Beside that order:

- **kiokun.com** is the owner's production target (ADR-0285). Its word page
  (labels, character header, written forms, examples, pitch accent,
  Contains and Appears in, homophones, deinflection) and its search page are
  built on the development server and held to kiokun.com's own code where
  it runs alone. ADR-0285's inventory counts 102 routes (none built, five
  partial) and 62 features (17 built, 10 partial, 35 missing as of
  2026-10-11). Static-data pages, accounts, in-browser features and outside
  services are not started. Its rows: search, static pages and SEO, routing.
- **TodoMVC** is in flight (`track/todomvc`), and D2's first app with no
  Rust data layer (0338). Its official Cypress suite passes 3 of 29 with
  the corrected gate, which fails it; D2 comes first.

**Rows no planned app covers yet**: video and media; experiments, flags and
analytics; AI token streaming. They decide the app after these. Interop and
testing tools for app authors are exercised by no reference app either.
Production is kiokun.com's, when the owner says go; a declared data layer is
the three-database app's; developer experience has its guide (0343), and
its editor support, `pw new` and live reload wait.
