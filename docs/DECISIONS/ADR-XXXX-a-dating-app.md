# ADR-XXXX: a dating app: whose a row is, who else may read it, and what a precise value reveals

Status: proposed for the integrator's ruling; a design note by track
`hinge-design` (the coordinator's brief of 2026-10-25). Docs only: nothing
here is built, and nothing was downloaded. Every version, licence and size
below was read from crates.io's API, from ffmpeg.org's and
download.geonames.org's HTTP headers, and from their pages; no crate,
tarball or dataset was fetched. Date: 2026-10-25, at master `cc47a64b`.
Milestone: E14, the app layer. Builds on ADR-0115 (a command's `requires`),
ADR-0129 (a value's label follows it), ADR-0207 (a source states its
guarantees), ADR-0258 and ADR-0263 (the relying party; a handle is the
platform's), ADR-0260 and ADR-0261 (uploads), ADR-0270 and ADR-0274 (the
reader's user; notifications), ADR-0279 (a conversation read from each
side), ADR-0302 (a predicate's words), ADR-0304 (the clock), ADR-0332
(payments), ADR-0334 (scheduled commands), ADR-0337 (a read is bounded),
ADR-0338 (an app's data declared, no app-specific Rust), ADR-0339 (the Docs
app, whose shape this note copies), ADR-0341 (reviews), ADR-0350 (D1,
tables), ADR-0354 (one snapshot a render), ADR-0366 (device-local storage)
and ADR-0369 (hosts share one database).

**The owner, 2026-10-25:** "we need to make various apps just to force
ourselves to complete the entire project so that it's completely done."
This note's job is to find, honestly, every capability a Hinge-shaped
dating app needs that Pleris lacks, and to lay out its build as tracks,
with no app-specific Rust.

## Ruling (the integrator, 2026-10-25)

Accepted under the owner's delegation of 2026-10-02 as the plan for a
fifth reference app, whose purpose is to make Pleris complete: each gap it
names is a framework track, never app-specific Rust. Answers to the
questions below:

1. **Codes**: the table clauses' refusals (PW6901 to PW6904, PW6909 to
   PW6913, PW6920) are declaration rules and go in `DeclarationRules`'
   PW03xx, next free after D1's PW0354 to PW0366, as ADR-0338's ruling 4
   placed D1's; the rest keep the PW69xx block for the H-tracks.
2. **ADR-0115 amended**: a program may give a predicate a body, written in
   Pleris, so who may act is in the program where the checker sees it.
   The deployment's Rust predicates (`identity.rs`'s `holds`) keep working
   until each is rewritten; the amendment states the order of asking.
3. **ADR-0279**: confirmed: two owners read from each side, and no label
   names two principals.
4. **ADR-0129**: confirmed narrow: a private field's write is a sink
   (PW6903) for private fields of owned tables alone; widened only with
   evidence.
5. **H1, H3, H4 and H9 wait for D2's writes** to land, then build on D2/D3's
   files; they are not folded into D2's open pull requests.
6. **One `outbound-https` platform track** serves NEXT 6d (payments), H7
   (push), H11 (verification) and the production track's real OIDC
   provider, behind the host's `network.fetch` (a program declares the
   origins; the host holds TLS and the keys). It waits for the owner's
   approval of its download (`rustls` 0.23.46 with `ring` 0.17.14, nine
   crates, and ISC admitted to `deny.toml`).
7. **The capability matrix** gains the four rows ("Location and distance",
   "Mutual-consent privacy", "Trust and safety: blocking, reporting,
   moderation", "Account deletion and data export") and the dating app as a
   reference app, at the next landing.
8. **The museum**: yes: the 2024 paper's three Hinge findings become
   exhibits by ADR-0342's protocol, shown in Next.js or SvelteKit; each is
   a miss in Pleris until H3's `reveals` and the policies that answer it
   are built, said so.
9. **`examples/dating/`** is the app's home: the fifth app, no Rust and no
   migrations of its own.
10. **Ports** are given to each track when it starts, from the browser host
    map's single port map.

Found, and fixed first on its own track: uploaded images keep their
metadata (ADR-0260), so a photo from a phone published in the feed today
tells where it was taken. That is a privacy defect of the framework as it
stands, not a gap of this app.

## The gaps, first

The main output. Each is examined below against Hinge's own pages and
Pleris's code at `cc47a64b`; the tracks that close them are in the build
plan.

1. **A declared table's rows carry no privacy.** ADR-0129: "a stored
   value's label is not tracked through storage". ADR-0341: the checker
   "holds what is read *through* a handle, not a user's id a data layer
   might answer". The feed's private data is private because hand-written
   Rust host operations take the reader's handle (ADR-0279); the
   no-app-specific-Rust rule (ADR-0338, ruling 1) removes exactly that
   Rust. A table's generated reads take plain ids, so a `public query`
   could read anyone's likes, skips or location; and a program with no
   Rust cannot even name its reader's own id, which the feed reads
   through Rust (`author(current_session())`). **(H1)**
2. **"Only after both liked" is not expressible.** "No label names two
   principals" (ADR-0279); a predicate's meaning is the deployment's Rust
   (`identity.rs`'s `holds`: `SignedIn`, `OwnsPost`, `MayMessage`,
   `MayRate`, ADR-0115); `requires` belongs to commands only, and
   ADR-0339's ruling 3 (`requires` on a query, accepted 2026-10-15) is not
   built. Hinge's Match Note is shown once both people have liked each
   other, and to matches alone. **Can the checker hold "only after both
   liked" today? No**: nothing in a program states it where the checker
   reads. With H1 to H3 it holds the structure (no read of the note but
   the generated one, which evaluates the predicate in the read's
   snapshot); what "matched" means stays the program's text, held by
   tests and mutation controls. **(H1, H2, H3)**
3. **Predicates cannot be written in Pleris.** A dating app has a dozen
   (matched, may like, may message, not blocked, under the turn limit, has
   a rose, subscribed, adult); each would be a Rust match arm in the
   deployment today. **(H2)**
4. **Nothing holds what a precise value may reveal.** Hinge's own bug,
   found in 2024: its distance *filter* used exact locations, so a filter
   answered whether someone was within a distance, an oracle that
   pinpointed users, though the distance was hidden in the UI.
   Pleris has no notion of a value that reaches another reader only
   coarsened; `device.Location` is `{ lat: Int, lng: Int }`, with no unit,
   and `current_location()` is `todo`. The same class: a birth date and an
   age, a last-active time and "active today", a like's time. **(H3, H5)**
5. **A read's rows are unbounded, and nothing pages by cursor.** A
   table's prefix and index reads answer every matching row (ADR-0350's
   functions take no limit); ADR-0337 bounds a read's calls, not its rows;
   pagination is "Load more reads again with a larger limit, with no
   cursor". Discovery, likes received and a conversation need "the newest
   N" and "the next N". **(H4)**
6. **Media is one public image.** One image a post; no audio or video;
   "an Exif GPS position is served as uploaded" (ADR-0260), so a profile
   photo would publish where it was taken (the feed does this today, see
   Found); a blob is served to anyone at its hash, so a blocked user who
   kept an address still loads the photos; no byte ranges, which Safari's
   media needs; no recording in the page (no camera or microphone effect).
   **(H6)**
7. **No push notifications.** No service worker, no Push API, and no
   outbound TLS, the gap the Stripe adapter waits on (NEXT 6d). **(H7)**
8. **Blocking, open since ADR-0279** (NEXT: "blocking, the integrator's"), has to
   hide one user from another everywhere: pages, `/pw-read`, the stream,
   caches, notifications and media. **(H1 to H3, A5)**
9. **No moderation**: "a comment is shown as written ... with no review,
   flagging, removal" (ADR-0341); no staff principal; no ban. **(H10)**
10. **No rate limits** but uploads' budget (capabilities.md, Production);
    a dating app is an abuse target, and limits are also its business
    rules (free likes a day, a rose a week). **(H8)**
11. **Nothing erases or exports a principal's data** across tables, blobs
    and sessions. **(H9)**
12. **No age assurance or selfie verification**, and no camera. **(H11)**
13. **Payments are one-off, against a development provider**: no
    subscriptions, entitlements or consumables. **(H12)**
14. **Forms bind a `String` alone**: no range, checkbox or `<select>`
    binding, which preferences and dealbreakers are made of. **(H13)**
15. **The data layer itself**: D2 has merged its reference adapter
    (`pw-table-memory`, PR #12) and is wiring read-only `get`/`get_many`
    through Wasmtime (draft PRs #29 to #40); writes, the outbox,
    idempotency, scheduled runs, migrations, the conformance suite and
    PostgreSQL (D3) are open (issue #27). The app waits for them.

## Context

### What the app is for

The owner's 2026-10-25 instruction makes apps instruments of completion.
This one covers rows no planned app covers: location and distance,
privacy between two principals that depends on what both did, trust and
safety (block, report, pause, delete), media past one image, and push. It
reuses what the feed and the store built: a principal, private queries,
live updates, notifications, messages, the clock, scheduled commands and
payments.

### Hinge, read from its own pages (2026-10-25)

**How it was read.** help.hinge.co answered this session's fetches with
HTTP 403 (bot protection); nothing was done to pass it. Its articles are
cited from the search engine's excerpts, marked *(help, excerpt)*: lower
confidence, to be re-read by a person before a ruling leans on one.
hinge.co's newsroom, its explainer, its terms and its privacy policy were
read directly. Quotes are short; the rest is paraphrase.

- **A profile**: four to six photos, by local requirements, and three
  prompt answers *(help, excerpt)*; a photo prompt pairs a photo with a
  prompt; one video prompt, "30-second videos" (newsroom, 2022-09-20); a
  voice prompt "for up to 30 seconds", shown under the first photo
  (newsroom, 2021-10-27); one prompt poll of three answers (2022-09-20).
  Vitals (name, age, height, location, ethnicity, family plans, pets,
  zodiac and more) each have a "Visible on Profile" box *(help, excerpt)*;
  a sexuality is required, "Prefer Not to Say" allowed *(help, excerpt)*.
  Top Photo: a model moves the photo most likely to be liked first, so the
  order others see may differ from the order edited *(help, excerpt)*.
- **Likes**: a heart on any photo or prompt, with an optional comment;
  free users send "a select number of likes per day" and see one incoming
  like at a time; subscribers send without limit and see all *(help,
  excerpt)*.
- **Roses and Standouts**: Standouts is a daily feed of profiles drawing
  attention; a Rose shows strong interest and puts its sender at the top
  of the recipient's Likes You; one free Rose a week *(help, excerpt)*.
  Virtual items are "FINAL AND NON-REFUNDABLE" (terms §8c, effective
  2025-08-25).
- **Discover** (hinge.co/how-we-connect-daters, undated, © 2026): a
  recommendation system of several algorithms, a deep-learning one since
  2025; preferences (age, distance, ethnicity, religion, relationship
  type; more for subscribers); two people are shown each other only "if
  you both meet the dealbreakers that either of you set"; "Prefer not to
  say" keeps a profile from those with a dealbreaker on that field; likes
  and quick skips teach it.
- **Location**: set from the device or a searched place, its pin movable;
  others see a neighborhood's name, not an address; not updated by
  itself; a maximum distance, optionally exceeded *(help "Location
  Settings", excerpt)*. The privacy policy (applies from 2025-11-24, §3)
  collects "your geolocation (latitude and longitude) from your device"
  with permission, and keeps an "approximate location" in profile data.
- **Matches and chat**: a match is a mutual like. Match Note (newsroom,
  2025-02-26): 255 characters, shown once both people have liked each
  other, to matches only, and viewed before the match completes. Your
  Turn Limits (newsroom, 2024-09-18): with "too many people waiting on
  their response" a dater replies or ends conversations before new likes;
  the match is not told *(help, excerpt)*. Chat-specific notifications,
  "Are You Sure?", "Did This Bother You?" and a comment filter of words,
  phrases and emoji on incoming likes (newsroom, 2025-11-10).
- **Ending things**: an unmatch is permanent, the conversation disappears
  and no further message can be sent; a report is "confidential and
  permanent" and neither sees the other again *(help, excerpt)*; a Block
  List hides a profile from people one knows (newsroom 2025-02-11; India,
  2026-09-28).
- **Pause**: no longer shown to new people, chats go on, likes sent
  before still arrive, a subscription is not paused *(help "Pausing",
  excerpt)*.
- **Verification and age**: Selfie Verification compares a video selfie
  with the profile's photos and badges the profile, and a photo changed
  can cost the badge *(help, excerpt)*; face data "is deleted within 30
  days" unless law or safety needs it (privacy policy §9). Age Checks: new
  UK users complete a biometric Face Check scan; Australia, Brazil, the
  EEA and Switzerland and Texas are listed *(help, excerpt)*. The terms:
  users are "at least 18 years old", and Hinge "does not conduct criminal
  background or identity verification checks" (§2a, §10).
- **We Met**: a few days after numbers are exchanged, a private question:
  did you meet, would you again; never shown to the match *(help,
  excerpt)*.
- **Subscriptions**: Hinge+ and HingeX: unlimited likes, every like seen,
  more preferences; HingeX adds likes kept near the top for seven days and
  a boost for the subscription's length *(help, excerpt)*. Renewal until
  cancelled; a 14-day refund in the EU, EEA, UK and Switzerland (terms §8).
- **Deletion and retention**: deleting an account deletes "all your
  content, including photos, chat history" *(help, excerpt)*; the privacy
  policy (§9) keeps a three-month safety window after closure (two years
  after a ban), transactions ten years, logs one year, and closes an
  account inactive for two years.
- **Moderation**: "a combination of automated tools and human reviewers",
  with appeals (terms §4).
- **Screenshot protection** in India on Discover, Likes You and Standouts
  (newsroom, 2026-09-28): a native app's; no browser can.

### What researchers found in Hinge (2024)

Dhondt, Le Pochat, Dimova, Joosen and Volckaert, "Swipe Left for Identity
Theft: An Analysis of User Data Privacy Risks on Location-based Dating
Apps", USENIX Security 2024. Read 2026-10-25 from the authors' copy
(lepoch.at; usenix.org refused this session's fetch), its text extracted
locally from the PDF:

- Of 15 apps, 6 allow pinpointing a victim's exact location.
- **Hinge's oracle**: in Badoo, Bumble, Hinge and Hily "the distance
  filters serve as the proximity oracle, as the filters use exact
  distances"; for Hinge and Hily despite distances hidden in the UI.
  Tinder and LOVOO resist by grid snapping.
- **Likes leak through answers the UI does not show**: Hinge, Badoo and
  Bumble "leak this in the user profiles of the card stack", against the
  user's perception that a free user must vote first; Hinge's API answers
  carry the like's timestamp.

Each is a bug of the kind Pleris claims to refuse: a comparison made on
a value its owner's audience never sees, and an answer carrying a field
the page hides. They are proposed as exhibits for the museum (ADR-0342;
questions).

### What Pleris has, read at `cc47a64b`

- **Tables** (capabilities.md, Missing, "A data layer declared in
  Pleris"; ADR-0338, ADR-0350): `table` with `row`, `key`, `order`,
  `index`, `unique`, `refers`, `bounded`, `migrate`; generated `get`,
  `get_many`, `of_<prefix>`, `by_<index>` and their `_many` forms, `all`,
  `insert`, `put`, `delete`, `delete_of_<prefix>`; a key, order or index
  field is a scalar; a row may hold lists, maps, records and sum types.
  D2: `pw-table-memory` merged (PR #12, 2026-10-24), the read path through
  Wasmtime in drafts #29 to #40; issue #27 lists what is open.
- **Privacy** (Built, "Privacy and per-user isolation"; ADR-0129,
  ADR-0263, ADR-0270, ADR-0279, ADR-0324, ADR-0341): labels follow values
  through calls; `user`, `session` and `public` say whose a query's answer
  is; a principal's handle is the host's alone to make; a private entry is
  told to its user's sessions alone. Not through storage, and nothing of a
  table's rows.
- **Predicates** (Built, "A refusal is never silent"; ADR-0115, ADR-0302,
  ADR-0307): declared with words in Pleris, evaluated by the deployment's
  Rust; `requires` on commands only (`check.rs`: "`requires` is an
  invocation precondition and belongs to a command").
- **Messages and notifications** (Built; ADR-0274, ADR-0279): the feed's,
  in its Rust layer; no blocking; no push, no email.
- **Uploads** (Built, "Uploads (images)"; ADR-0260, ADR-0261): PNG, JPEG,
  WebP and GIF, sniffed, sized, one a post; served at
  `/images/<sha256>`, `public, immutable`, while a committed row names it,
  which the layer answers in Rust (`DataLayer::names_blob`); Exif GPS kept;
  no byte ranges (the server's code has no `Content-Range`).
- **The device**: `effect device.location` (browser placement) and
  `device.current_location()`, a `todo`; no camera, microphone or
  notification effect; no service worker anywhere in the runtime.
- **The clock and schedules** (Built; ADR-0304, ADR-0334): `passed`,
  `today_in`, a kept value told when the clock passes what it compared; a
  command scheduled once, run as the user who scheduled it; no recurring
  schedule.
- **Payments** (Partial; ADR-0332): a development provider; signed
  webhooks invoke a command; no real provider, refunds or subscriptions.
- **Forms** (Partial): `bind:value` on a `String`; TodoMVC's checkboxes
  are a `checked` attribute and a handler.
- **Accounts** (Partial): a development identity provider; Google's OIDC
  queued (NEXT); no email or phone verification.
- **Production** (Missing): no TLS, no rate limit but uploads' budget.
- **Hosts** (ADR-0369): hosts share one database, sessions in it.

### How others hold row-level privacy (2026-10-25)

- **PostgreSQL 18, row security policies** (docs, "Row Security
  Policies"): a policy's `USING` decides which rows a command sees and
  `WITH CHECK` which rows it may write; with security enabled and no
  policy, "no rows are visible or can be modified". A policy may read
  other tables, and the documentation warns that such a read can race
  under `READ COMMITTED`, letting a user briefly see what they should not.
- **STORM** (Lehmann et al., OSDI 2021; abstract read on papers.cool,
  usenix.org refusing the fetch): policies declared beside the data model,
  enforced at compile time by refinement types, at no run-time cost, the
  trusted code under 1% of an application.
- **What this note takes**: the default deny and the write check from the
  first; the policy declared beside the schema and held at compile time
  from the second; not refinement types. Pleris already makes a table's
  generated functions the only way to its rows (PW0362), so a policy
  compiled into those functions is one no program can step around, and
  the first one's race does not arise when the policy is read in the
  read's own snapshot (ADR-0354).

## The gaps, feature by feature

"None" is the matrix's Missing: nothing of it, or pieces that do not add
up to it.

| Hinge's feature | What Pleris has (matrix row; ADRs) | What it lacks | Track |
| --- | --- | --- | --- |
| Four to six photos, in order | Uploads, built: one image a post (0260) | several media in a row and their order; Exif stripped; served only to permitted readers; collected by a generic rule | H6 |
| Three prompts; a poll | rows hold lists and sum types (0350) | nothing beyond H1's privacy | A1 |
| Voice and video prompts, 30 s | none ("Video and media", Missing) | audio and video kinds, duration, byte ranges, recording in the page | H6 |
| Vitals, each visible or hidden | none | a field its owner shows or hides, and others' view without it | H1 |
| A like on a photo or prompt, with a comment | the feed's like, in Rust (0274) | a like private to its sender and its recipient, declared | H1, A3 |
| Likes You: one at a time, or all | none | the recipient's view of likes (`shares to`); one at a time is the app's rule in its query, not a privacy rule | H1, A3 |
| Roses, Standouts (daily) | the clock's day (0304) | a balance and a weekly grant (H12 for purchases) | A3, H12 |
| Discover, preferences, mutual dealbreakers | `List.sort_by`, `List.filter` | candidates near the reader, bounded; others' preferences used, never shown | H3, H4, H5, A2 |
| A neighborhood and a distance | `device.location` declared, `todo` | the browser's read; a coarse cell; a distance between cells; a precise value never compared | H5, H3 |
| A match on a mutual like | serializable commands (0207, 0325) | the tables' writes (D2), PostgreSQL (D3) | D2, D3, A3 |
| Match Note, matches only | none | a row readable when a predicate holds of its owner | H2, H3 |
| Chat | DMs, built in Rust (0279) | the same, declared; read from each side | H1, H4, A4 |
| Your Turn Limits | none | a predicate in Pleris refusing a like | H2, A4 |
| Unmatch, hide, remove | none | two-owner rows deleted by either owner; the actor's block row, read both ways by every predicate | H1, H2, A4 |
| Block (people one knows) | none (NEXT, after 0279: "blocking, the integrator's") | hidden everywhere; by phone or email needs verified identifiers | H1 to H3, A5 |
| Report | none (0341: no moderation) | a report, a staff principal, review, bans | H10, A5 |
| Pause | none | a policy's conjunct | H3, A5 |
| Selfie verification, age checks | payments' provider pattern (0332) | a provider, the camera, biometric data never kept | H11 |
| We Met | scheduled commands (0334) | each user's run scheduled by their own action | A4 |
| Notifications: in-app, push, per chat | in-app, built (0274) | push; per-conversation settings | H7, A6 |
| Subscriptions, roses, boosts | a development provider (0332) | recurring billing, entitlements, consumables | H12, A7 |
| Delete account; retention | none | generated erasure and export; a hold | H9 |
| Comment filter, Are You Sure? | none | a word list in Pleris; scanning providers not claimed | A3 |
| Screenshot protection | not possible in a browser | not claimed | none |
| Sign in, 18+ | development provider (0258) | a real provider (NEXT); a birth date held | A1 |

## Decision

### 1. A table says whose its rows are, who else may read them, and what they see

A sketch; syntax provisional (ADR-0010):

```
table Likes
    row     Like
    key     to, from
    index   by_from_day on from, day
    owner   from             // the sender's: read, written and erased with them
    shares  to               // and its recipient's to read
    written when MayLike(to) // a row another reads is written only when this holds
    private day, sent        // read by its owner alone
```

- **`owner <field>`** names the field, a principal's id (`UserId`), whose
  row it is. Each generated read that leads with the owner field takes the
  reader's handle (`capability.User<UserId>`) in that field's place:
  `Likes.by_from_day(me, day)`. A program makes no handle (ADR-0263), so it
  can read no other user's rows by naming an id. The adapter compares the
  handle's user with the field; the answer is labelled the reader's.
- **The reader's own id**: a row names its owner by `UserId`, so a
  program writing its reader's like needs that id as a value. The platform
  gives none today (the feed reads it through Rust,
  `author(current_session())`); `capability.id_of(current_user())` is
  added, an id that grants nothing (ADR-0270), labelled the reader's.
- **Two owners** (`owner low, high`) are a pair's rows, read from each side
  (ADR-0279's ruling, kept): each owner reads them as their own, and the
  generated read of the pair takes the reader's handle and the other's id.
- **`shares <field>`**: the principal another id field names reads the row
  too, as their own (`Likes.of_to(me)`: likes sent to me).
- **`shares when <Predicate>`** (decision 2's predicates): a reader for
  whom the predicate holds of the row's owner reads it,
  `Profiles.seen(me, user)` and its set form `seen_many(me, users)`, the
  predicate evaluated in the read's snapshot, in its set form for a set
  read.
- **`private <fields>`**: what the owner alone reads. Every other reader
  is answered the **seen row**, a record the compiler generates beside the
  table (`LikeSeen`) without those fields, and with each `reveals` field
  (decision 3) in their place. A value another reader may not see is not
  in the type they are answered, so no page, frame, `/pw-read`, cache entry
  or `EntryValue` can carry it: the like's time the paper found in
  Hinge's answers has no field to leak through.
- **Default deny**: a table with an `owner` and no `shares` is its owner's
  alone; a table with no `owner` is the program's public data (the prompt
  catalogue), and a table holding a `UserId` field states one or the
  other (H9's PW6920).
- **Writes are checked as PostgreSQL's `WITH CHECK`**: an owned row is
  inserted, put or deleted only through an owner's handle (its owner's;
  either owner's of a two-owner row), the adapter checking the owner
  field against it. **No row is written for another**: what two
  principals share is a two-owner row either writes (a match, a
  conversation, whose turn it is); what one tells another is a row the
  teller owns and shares (a like).
- **`written when <Predicate>`**: a row another principal will read (a
  `shares <field>` row, or a two-owner row) is written only when a
  predicate over that other principal holds, evaluated by the adapter on
  every write in the command's transaction, so no command can forget it;
  a table whose rows another reads and that states none is refused (H3's
  PW6911). Without it, anyone could open a conversation with anyone by
  writing a two-owner row naming them.
- **Erasure and export come from the same clauses** (H9): every row a
  principal owns, in every table of the program's sources, is found by its
  owner fields; no Rust lists them.
- **What the checker then holds**: a query cached shared reads no owned
  table (PW6902, PW5001's rule for tables); a private field's value is
  written into no row another may read, and returned in no answer but its
  owner's (PW6903: for a private field, a write becomes a sink, amending
  ADR-0129's "a write is not a sink" there alone).

### 2. A predicate may be written in Pleris

```
predicate Matched(other: UserId)
    says "You can see this once you've both liked each other."
{
    present(Matches.pair(current_user(), other)) & !blocked_either(current_user(), other)
}
```

(`Matches` is a two-owner table; `pair` its generated read of one pair
from the reader's side; `present` is `Option`'s "is `Some`", which the
standard library lacks today.)

- **A body reads, and does nothing else**: no write, no schedule, no
  network, no clock but `clock.compare` (PW6905). It runs before the
  command's body, as `requires` does today (ADR-0115), inside the
  command's transaction, so what it read is what the command writes
  against (ADR-0258's `OwnsPost` reasoning, kept).
- **A body reads with the policy's authority, not the reader's.** Whether
  the other blocked the reader is the other's row, which the reader may
  not read (decision 1); a predicate must read it. So inside a
  predicate's body alone, a table's reads take ids in the owner's place
  (`Blocks.get(other, me)`), as a PostgreSQL policy reads what its
  caller cannot; elsewhere they take the handle. What a body read leaves
  it as one bit, the predicate's answer, which its use discloses anyway
  (a blocked profile is not shown); the checker holds that nothing else
  of what it read flows out (the body's result is a `Bool`, its reads
  reach no write, log or answer).
- **Its parameters are its uses' parameters** (PW6906): ADR-0115's rule
  that "arbitrary expressions are not run before authorization" stands
  for arguments; the body is the program's authorization, evaluated by
  the host as a compiled export. A predicate the program does not give a
  body is the deployment's, as now.
- **A predicate may call another**, and a body's helper functions are
  held to the same rules (`blocked_either` above reads `Blocks` both
  ways, which only a body may).
- **`requires` on a query** (ADR-0339's ruling 3, its code PW6907): a page
  whose query a predicate refuses is told the predicate's words (ADR-0302).
- **A predicate guarding a set read states its set form** (PW6908), held
  by PW0405's rule inside its body, so a policy over 200 candidates is a
  set read, not 200 reads.
- **This amends ADR-0115's first decision** ("predicate names ... belong
  to the deployment's authorization vocabulary, not to the Pleris term
  namespace") for predicates a program declares with a body: a question
  for the integrator.

### 3. A precise value reveals only what its table declares

- **`reveals <field> as <function>`**: a private field another reader
  sees only through a declared pure function, computed inside the
  generated seen read, so the precise value never leaves it:

  ```
  table Profiles
      row     Profile
      key     user
      owner   user
      index   by_cell on cell
      shares  when Discoverable(user)
      private birth, paused, last_active
      reveals birth as age_on, last_active as active_today
  ```

  The seen row has `age: Int` and `active_today: Bool` in place of `birth`
  and `last_active` (the whole declaration, vitals included, is decision
  9's). A function whose result is the field's own type (an identity
  "coarsening") is refused (PW6910).
- **A platform `geo` module**, generic, its arithmetic the host's
  intrinsics (Pleris has `Float` but no trigonometry):

  ```
  opaque type Precise          // a device's reading, decoded at a command's boundary
  opaque type Cell = String    // a cell of a fixed grid; an index's field
  fn cell_of(p: Precise) -> Cell !{}
  fn near(c: Cell, km: Int) -> List<Cell> !{}   // at most a declared number of cells
  fn apart(a: Cell, b: Cell) -> Int !{}         // km between the cells' centres, rounded up to a step
  ```

  `Precise` has no comparison and no distance: the only thing a program
  can do with it is `cell_of`, store it in a `private` field, or drop it.
  This app drops it (data minimisation, among the paper's countermeasures): its
  filter and its display both use `apart(cell, cell)`, so no comparison
  whose answer reaches another reader is made on the precise point, the
  2024 oracle's cause. A cell is revealed by design, as Hinge reveals a
  neighborhood; a finer oracle than a cell is not possible, and a cell's
  size is an owner decision.
- **A disclosure by design is declared too.** Mutual dealbreakers use the
  candidate's private preferences to decide whether the reader sees them:
  one bit per candidate, which a reader could probe by editing their own
  profile. Declared, `Preferences ... reveals dealbreakers as accepts`
  (where `accepts(prefs, reader_seen_profile) -> Bool`), the bit is in one
  reviewable place, and the facts a reader could vary to probe it (birth
  date, location) are rate-limited (decision 8).
- **A like's time is private** to its sender; the recipient sees likes in
  their order (H4's cursors carry no order value, below).

### 4. A read's rows are bounded, and its pages are cursors the host makes

- Each prefix and index read gains `first(n)` and `after(cursor, n)`,
  in the table's `order`, which may be `descending`; `n` is at most a
  bound the read declares (PW6912).
- **A cursor is a handle the host makes** (PW6913, ADR-0263's rule for a
  new handle): opaque, its read's label, and carrying no order value, so
  "the next 20 likes" does not tell a recipient when each was sent.
- This is ADR-0337's other half: it bounds a read's calls; this bounds
  its rows, and a page's budget becomes calls and rows.

### 5. A row may hold media: several, in order, stripped, private

- **`Media<K>`** is a field type (`K` an `upload` declaration), so a
  profile's photos are `List<Photo>` in its row, their order the list's,
  a reorder one `put`. The adapter knows from `tables.json` which rows
  name which blob, so ADR-0261's "served while a committed row names it"
  becomes generic, replacing each layer's Rust `names_blob`.
- **Metadata stripped before a byte is kept**: JPEG's APP1 (Exif, XMP),
  PNG's `eXIf` and text chunks, WebP's `EXIF` and `XMP` chunks, an MP4's
  `udta` and `meta` boxes (where phones put a recording's place), WebM's
  `Tags`; the host's own code, no download.
- **Served only to a reader the row's policy admits**: no public,
  immutable address; a session-checked route (or a short-lived address the
  host signs), so a block, an unmatch, a pause or a deletion stops the
  media with the rest.
- **Audio and video**: `upload` gains `mp4`, `webm` and `ogg` kinds by
  WHATWG MIME Sniffing's §6.2 (its MP4 and WebM signatures; the standard
  last updated 2026-10-19), and `max_seconds`, read from the container
  (its stated duration, or its last frame's time where a recorder states
  none, which is measured, not assumed); byte ranges served (RFC 9110),
  since "HTTP servers hosting media files for iOS must support byte-range
  requests" (Apple's Safari Web Content Guide, updated 2016-12-12).
- **Recorded in the page**: `device.camera` and `device.microphone`
  effects (browser placement), MediaRecorder behind them; which formats
  each engine records and plays is measured first, not assumed. No
  transcoding by default (owner decision 4).

### 6. The browser's device: location and notifications

- **Location**: `device.current_location()` implemented over the
  Geolocation API, asked in a handler a press started (PW6915: a device
  permission is asked from a press, never on load), decoded into
  `Precise` (latitude and longitude in range) and sent as a command's
  argument. Playwright emulates a position and grants the permission
  (playwright.dev, Emulation); each engine is measured.
- **Push**: a service worker the build generates (same origin, allowed by
  ADR-0333's policy as a `worker-src 'self'`); `device.notify`, asked from
  a press; the subscription held by the platform, since its endpoint is "a
  capability URL" anyone holding it can send to (MDN, Push API, modified
  2025-05-28); messages encrypted by RFC 8291 (ECDH P-256, HKDF,
  AES-128-GCM) and signed by RFC 8292 (VAPID, a P-256 JWT), both with the
  `ring` crate the TLS client brings (downloads); delivered from the
  outbox by a durable queue with retries; preferences per user and per
  conversation. On iOS, push reaches Home Screen web apps only, from iOS
  16.4 (WebKit blog, 2023-02-16): said on the page.
- **A push carries no other principal's private field by default**
  (PW6918): a lock screen is read by whoever holds the phone, so a
  message's text is a preview its reader turns on.

### 7. Safety is the sharing predicate's

- **Block**: `Blocks` (`owner by`, shared with no one: a block is
  confidential). Every `shares when` predicate of the app conjoins
  `NotBlocked(owner)` both ways, so no read path bypasses a block; the
  block's events drop the other's entries in the blocked user's sessions,
  so their open pages lose the profile, the match and the conversation,
  and their media stops (decision 5).
- **Report**: a row owned by its reporter, shared with the deployment's
  moderators (a staff principal, H10), and a block both ways at once, as
  Hinge's report is.
- **Pause**: `paused` is private; `Discoverable` excludes a paused owner
  except to their matches.
- **Unmatch**: the pair's match, conversation and turn rows deleted in
  one transaction, and the actor's `Blocks` row written (its `why`
  "unmatched"); since every predicate reads blocks both ways, neither is
  shown the other again.
- **Delete account** (H9): every owned row, every blob only those rows
  named, the sessions and the scheduled runs; a two-owner row goes for
  both, as Hinge's deleted chat history does; a safety hold is owner
  decision 9.

### 8. Limits are declared

- **A command's `rate`** (`rate 200 per 1.days per user`, PW6919): a
  counter in the platform's tables, one across hosts (ADR-0369), refused
  429 with `Retry-After` and told in the page's words (ADR-0302).
- **Business quotas are predicates** over the tables (free likes a day,
  the turn limit), so they are refused with words where the press was.
- **What would let a reader probe a disclosure** (decision 3) is limited:
  a location changed at most a few times a day; a birth date set once.

### 9. The app, declared

A sketch of `examples/dating/` (syntax provisional): no Rust and no
migrations of its own (ADR-0339's ruling 2). Each table is declared in a
module of its name (ADR-0350: one table a module), so the app is a module
per table, the source's module and its pages' module.

**Its tables**, in one source, `DatingData`:

| Table | Key | Whose; who else reads; written when | Private (revealed as) | Index, order |
| --- | --- | --- | --- | --- |
| `Profiles` | `user` | `owner user`; `shares when Discoverable(user)` | `birth` (`age_on`), `last_active` (`active_today`), `height` and `hometown` (`shown`), `paused` | `by_cell on cell` |
| `Preferences` | `user` | `owner user` | all (`dealbreakers` as `accepts`) | |
| `Likes` | `to, from` | `owner from`; `shares to`; `written when MayLike(to)` | `day`, `sent` | `by_from_day on from, day` |
| `Skips` | `from, to` | `owner from`; no one | | |
| `Matches` | `low, high` | `owner low, high`; `written when LikedBy` | | `by_high on high` |
| `Messages` | `low, high, sent, id` | `owner low, high`; `written when Matched` | | `order sent descending` |
| `Turns` | `low, high` | `owner low, high` (whose turn); `written when Matched` | | `by_turn on turn` |
| `MatchNotes` | `user` | `owner user`; `shares when Matched(user)` | | |
| `Blocks` | `by, other` | `owner by`; no one (`why`: unmatched, blocked, reported) | | `by_other on other` |
| `Reports` | `by, id` | `owner by`; staff (H10) | | |
| `WeMet` | `by, other` | `owner by`; no one | | |
| `Wallets` | `user` | `owner user` (roses; the week's free one) | | |
| `Entitlements` | `user` | `owner user`; written by a `Payments` command alone | | |
| `Prompts` | `id` | none: public, `bounded 500` | | |

Two in full, as D1's grammar writes a table, each in its own module:

```
module Messages

import capability.{ UserId }
import Matches

opaque type MessageId = String
opaque type MessageText = String where String.length(value) >= 1 & String.length(value) <= 2000

type Message = Message { low: UserId, high: UserId, sent: Instant, id: MessageId,
    from: UserId, text: MessageText }

table Messages
    row     Message
    key     low, high, sent, id
    order   sent descending          // H4
    owner   low, high                // a pair's: read from each side (ADR-0279)
    written when Matched             // over the other owner (H3)
```

```
module Profiles

type Visible<T> = Visible { value: T, shown: Bool }   // a vital its owner shows or hides
type Photo = Photo { id: PhotoId, media: Media<ProfilePhoto>, prompt: Option<PromptId> }
type Answer = Answer { prompt: PromptId, text: AnswerText }

type Profile = Profile {
    user: UserId, first_name: FirstName, birth: LocalDate, gender: Gender,
    height: Visible<Centimetres>, hometown: Visible<String>,
    photos: List<Photo>, answers: List<Answer>,                 // their order is the list's
    voice: Option<Media<VoiceAnswer>>, video: Option<Media<VideoAnswer>>,
    cell: Cell, paused: Bool, last_active: Instant, verified: Bool,
}

table Profiles
    row     Profile
    key     user
    owner   user
    index   by_cell on cell
    shares  when Discoverable(user)
    private birth, paused, last_active, height, hometown
    reveals birth as age_on, last_active as active_today, height as shown, hometown as shown
```

(`shown` turns a `Visible<T>` into an `Option<T>`: `None` where its owner
unticked "Visible on Profile".)

**Its rules**, predicates with bodies (decision 2), read with the
policy's authority:

```
predicate NotBlocked(other: UserId) says "This profile isn't available."
{
    !blocked_either(current_user(), other)
}

predicate Discoverable(user: UserId) says "This profile isn't available."
{
    NotBlocked(user) & (!paused(Profiles.get(user)) | present(Matches.pair(current_user(), user)))
}

predicate LikedBy(other: UserId) says "You can match once they've liked you too."
{
    present(Likes.get(current_user(), other))        // `other` liked the reader
}

predicate MayLike(to: UserId) says "You can't like this profile right now."
{
    Discoverable(to) & UnderTurnLimit() & (Subscribed() | LikesLeftToday())
}

predicate MayMessage(other: UserId) says "You can message someone once you've matched."
{
    Matched(other)
}
```

**Its reads and writes** (a sketch):

```
event Liked(from: UserId, to: UserId)
event Paired(a: UserId, b: UserId)
event Messaged(from: UserId, to: UserId)
event Ended(by: UserId, other: UserId)               // an unmatch, a block, a report

// The reader's discovery: candidates in the cells near them, as each is
// seen, whom the reader has not acted on, who accept the reader and whom
// the reader's preferences accept, ranked by the program's own score;
// bounded throughout (H4), set reads throughout (PW0405).
user query Discover(reader: capability.User<UserId>) -> List<Card>
    cache          private
    key            reader
    invalidates_on Liked(reader, _), Skipped(reader, _), Ended(reader, _), Ended(_, reader),
        Moved(reader)
{
    let me = Profiles.get(reader)
    let prefs = Preferences.get(reader)
    let near = Profiles.by_cell_many_first(reader, geo.near(me.cell, prefs.max_km), 500)
    let liked = Likes.get_many(reader, ids(near))           // the reader's own likes of them
    let skipped = Skips.get_many(reader, ids(near))
    let accepts = Preferences.accepts_many(reader, ids(near))
    rank(prefs, me.cell, List.filter(near, c => fits(prefs, c) & accepted(accepts, c)
        & fresh(liked, skipped, c)))
}

user query LikesYou(reader: capability.User<UserId>) -> LikesYou      // all, or one and a count
user query Matches(reader: capability.User<UserId>) -> List<MatchItem>
user query Conversation(reader: capability.User<UserId>, other: UserId, page: Option<Cursor>)
    -> Result<Conversation, DatingError>
    requires MayMessage(other)                                       // decision 2

// A like on a part of a profile, a comment or none; a match where the other
// already liked the reader, in the same serializable transaction, so two
// likes at once make one match and never none: a write skew under
// snapshot isolation, which `transactions serializable` refuses.
command like(to: UserId, part: Part, comment: Option<Comment>) -> Result<Liked, DatingError>
    requires      SignedIn, MayLike(to)
    rate          500 per 1.days per user                            // decision 8
    idempotent_by InteractionId
    emits         Liked(id_of(current_user()), to)

command skip(user: UserId)                        -> Result<(), DatingError>
command unmatch(other: UserId)                    -> Result<(), DatingError>    // requires Matched
command block(other: UserId)                      -> Result<(), DatingError>
command report(other: UserId, why: ReportReason)  -> Result<(), DatingError>
command send(other: UserId, text: MessageText)    -> Result<Message, DatingError> // requires MayMessage
command set_place(here: Precise)                  -> Result<(), DatingError>    // keeps cell_of(here)
command pause(), unpause(), delete_account(), answer_we_met(other: UserId, met: Bool)

session page DiscoverPage()          { route "/" }                 // a card, its parts, Like and Skip
session page ProfilePage(user: UserId) { route "/u/{user}" }       // the seen profile
session page LikesPage()             { route "/likes" }
session page MatchesPage()           { route "/matches" }
session page ChatPage(other: UserId) { route "/matches/{other}" }
session page EditPage()              { route "/me" }               // photos in order, answers, vitals
session page SettingsPage()          { route "/settings" }         // preferences, pause, delete, push
```

- **Discovery is bounded by construction**: on a `one_call` source its
  reads are a fixed number of set reads (the reader's rows, the
  candidates by cell, the reader's likes and skips of them, their
  preferences' answer, and the policy's own set reads of blocks and
  matches), whatever the number of candidates, and at most 500 rows by
  `first`: ADR-0337's calls and H4's rows, both held.
- **The rank is the program's**, a deterministic score over the seen
  candidates (mutual preference fit, distance band, recent activity), not
  Hinge's model. Standouts is the same pool under another score, keyed by
  the reader's day (`today_in`), so it changes when the day does
  (ADR-0304) with no recurring job.
- **We Met** is scheduled by each user's own first message in a
  conversation (`schedule ask_we_met(other) after hours(96)`), since a run
  is its scheduler's (ADR-0334).
- **Your Turn Limits** read `Turns`, which `send` keeps (the turn passes
  to the other), and `UnderTurnLimit()` reads the reader's turns with
  `Turns.by_turn_first(me, limit + 1)`, bounded (H4).
- **A block, a report or an unmatch** writes the actor's `Blocks` row
  (with its `why`), deletes the pair's `Matches`, `Messages` and `Turns`
  rows where there are any, and emits `Ended(me, other)`, which drops the
  other's entries in their sessions: their open pages lose the profile,
  the match, the conversation and the photos at once.

### 10. What the checker would refuse

| A program that | Is refused by | Why it matters here |
| --- | --- | --- |
| reads `Likes`, `Skips` or `Preferences` in a `public query` | PW6902 | another user's acts in a shared cache |
| returns another user's `birth`, `paused` or a like's `sent` | the seen type has no such field | the paper's API leaks |
| filters candidates by a distance on `Precise` | `Precise` has no distance; it is never stored | Hinge's 2024 oracle |
| reads a conversation it is not a side of | the generated read takes the reader's handle | a third user's view |
| shows a Match Note without `Matched` | `shares when Matched(user)` is in the read | shown once both have liked, to matches alone |
| writes a message to someone it has not matched | `Messages`' `written when Matched`, held on every write; a table another reads with no `written when`, PW6911 | a conversation opened with anyone |
| asks for the location on load | PW6915 | a prompt the reader did not start |
| writes a reader's `birth` into a match's row | PW6903 | a private field copied where others read |
| keeps an `Instant` of a like in a cursor | PW6913 | the order value leaking through paging |
| puts a message's text in a push | PW6918 | a lock screen read by anyone |
| declares a table holding a `UserId` with no `owner` or reason | PW6920 | erasure incomplete |

## How it fits Pleris

| Concern | The compiler checks | The runtime does |
| --- | --- | --- |
| Whose a row is | `owner` names a principal's id field; generated reads lead with the reader's handle; a shared cache reads no owned table | the adapter compares the handle's user with the owner field; default deny |
| Who else reads | `shares <field>`; `shares when P`, P a Pleris predicate with a set form | P evaluated in the read's snapshot, per row or by set; failing rows not answered |
| What others see | the seen row: `private` fields absent, `reveals` fields computed by declared pure functions | the generated read builds it; the private value never leaves the read |
| Writing | owned rows through an owner's handle; a private field written into no shared row; `written when` on every table another reads | the owner field and the `written when` predicate checked on every write |
| Predicates | a body that only reads; parameters; words (PW0351) | evaluated in the command's transaction or the read's snapshot; refused with words |
| Location | `Precise` has no operation but `cell_of`; a device read only from a press | the browser asks; the host decodes; the grid's intrinsics |
| Rows | `first(n)`, `after(cursor, n)`, n bounded; cursors are handles | keyset reads; cursors made and checked by the host |
| Media | `Media<K>` fields; kinds and seconds; capture from a press | metadata stripped; served to permitted readers; ranges; collected when unnamed |
| Push | no other principal's private field in a payload | subscriptions held by the platform; RFC 8291 and 8292; delivered from the outbox |
| Limits | `rate`'s shape | counters across hosts; 429 with words |
| Erasure | every table naming a user states its owner or why it keeps one | erase and export generated from the owners |

- **The privacy model is extended, not replaced**: a row read through a
  handle is the reader's, as the feed's private queries are; two owners
  read from each side, as ADR-0279 ruled; what is new is that the policy
  lives in the table's declaration, where the checker sees it, and in
  the generated functions, where no program can step around it.
- **Placement**: device effects in the browser; predicates and seen reads
  at the origin with the data; the push worker in the browser.

## The build plan, as tracks

Each track: its files, its codes (proposed; PW69xx, free on all 162 origin
refs at 2026-10-25, the integrator assigning), its recipe
(`just/<track>.just`, `e14-<name>` writing `docs/evidence/E14/<name>.txt`),
its acceptance and its mutation controls (`scripts/<track>_mutations.py`,
each `.pw` mutant still well typed), and the capability-matrix row it
moves. In order of what each needs.

### Phase 0: what other tracks owe first

- **D2 `memory-adapter`** (ADR-0338; issue #27's open list), before any
  H track's runtime half: `insert`, `put`, `delete` and `delete_of` in a
  command's transaction, rolled back with it; a command's `emits`
  committed with its table writes (the outbox: every live update this
  app shows); `idempotent_by`'s record committed with them (a like or a
  message sent twice is one); a `schedule` committed with them (We Met, a
  subscription's lapse); the set reads through Wasmtime (drafts #29 to
  #40 carry `get` and `get_many`; the prefix and index forms follow); the
  generated conformance suite. Built already in `pw-table-memory`:
  uniqueness, multi-field keys, references in one source, rollback.
- **D3 `postgres-adapter`**, before the app's slices: migrations from
  `tables.json` under the advisory lock; serializable transactions (two
  likes at once must make one match, a write skew below serializable);
  the platform's tables shared by hosts (ADR-0369's outbox and sessions,
  for tables); drift refused at start. The H tracks' compiler halves do
  not wait for it.
- **`outbound-https`** (new; shared with NEXT 6d's Stripe adapter, H7,
  H11): a TLS client in the host behind `network.fetch`'s grant, its
  roots the operating system's, timeouts bounded; downloads 1.

### Phase 1: the framework, generic

**H1 `table-owners`: whose a row is, who else reads it, what they see.**
- Files: `compiler/pw-syntax/src/grammar.rs` (`owner`, `shares`,
  `private`), `compiler/pw-core/src/tables.rs` (handle-taking reads, the
  seen row, write checks, `tables.json`'s owners), `privacy.rs` (PW6902,
  PW6903), `codes.rs`; `packages/pw-platform-web/capability.pw`
  (`id_of`); `pw-table-memory` (the owner check, the seen row);
  `compiler/pw-core/tests/table_owners.rs`; corpus fixtures.
- Codes: PW6901 `owner`/`shares` name a field holding a principal's id;
  PW6902 a query cached shared reads an owned table; PW6903 a private
  field written into a row another may read, or returned to another;
  PW6904 a two-owner table's key does not lead with both owners.
- Acceptance (`e14-table-owners`): each rule refused beside its control;
  the generated signatures (a handle in the owner's place; `LikeSeen`
  without `day` and `sent`); on memory, a read by another user's handle
  answering nothing, a write with another's owner field refused, a
  two-owner pair read from each side; `tables.json` stating owners.
- Mutation controls: the owner check skipped; a private field kept in the
  seen row; a write's owner not compared; PW6902 and PW6903 each disabled.
- Moves: "Privacy and per-user isolation" (its "not through storage"
  limit); "A data layer declared in Pleris".

**H2 `predicates-in-pleris`: a predicate's body, and `requires` on a
query.**
- Files: `compiler/pw-core/src/predicates.rs` (bodies, set forms),
  `check.rs` (`requires` on a query), `contract.rs` (a predicate an
  export), `runtime/pw-host` (evaluating a compiled predicate in the
  command's transaction or the read's snapshot), the server's `requires`
  path calling it before `identity.rs`'s deployment match;
  `compiler/pw-core/tests/predicate_bodies.rs`; a browser spec of a query
  refused with words.
- Codes: PW6905 a predicate's body does anything but read; PW6906 its
  arguments are its uses' parameters; PW6907 a query's `requires`; PW6908
  a predicate guarding a set read has no set form.
- Acceptance (`e14-predicates-in-pleris`): each rule refused beside its
  control; a fixture program over declared tables (`examples/tables/`'s
  kind) whose commands and a query require predicates with bodies,
  refused and allowed as their bodies say, on memory (and PostgreSQL
  after D3); a predicate's reads inside the command's transaction (a
  concurrent change between predicate and body refused by
  serializability); a page whose query is refused showing the words, in
  three engines. The feed's `MayMessage` and the store's `MayRate` move
  to bodies when D4 and D5 declare their data, and their Rust arms in
  `identity.rs` go then.
- Mutation controls: the predicate skipped; inverted; evaluated outside
  the transaction; a write allowed in a body; the query's `requires`
  ignored.
- Moves: "A refusal is never silent" (predicates with parameters);
  "Privacy"; "A data layer declared in Pleris" (no predicate in Rust).

**H3 `table-policies`: `shares when`, `written when` and `reveals`.**
Needs H1, H2.
- Files: `tables.rs` (the guarded reads and their set forms, the write
  guard, the revealed fields), `pw-table-memory`, D3's adapter, tests.
- Codes: PW6909 `shares when` and `written when` name a predicate with a
  body, over one principal, whose body does not read the table it guards
  (which would recurse); PW6910 `reveals` names a pure function whose
  result is not the field's own type; PW6911 a table whose rows another
  principal reads (`shares <field>`, or two owners) states no `written
  when`.
- Acceptance (`e14-table-policies`): a Match Note unread before the
  match and read after it, in one transaction's snapshot and the next;
  a seen profile with `age` and without `birth`; a set read of 500
  candidates under `Discoverable` costing its set reads, counted at two
  sizes; a block making every guarded read of the pair answer nothing;
  a message written to an unmatched user refused by the adapter, whatever
  the command.
- Mutation controls: the guard skipped on the set form only; the
  predicate read outside the read's snapshot; `reveals` answering the raw
  field; the write guard skipped; PW6911 disabled.
- Moves: "Privacy"; a new row, "Mutual-consent privacy" (question 7).

**H4 `bounded-rows`: `first`, `after`, descending, cursors.**
- Files: `tables.rs` (the forms, `order ... descending`), `pw-table-memory`
  and D3's adapter (keyset reads), the platform's `Cursor` handle, tests;
  the feed's "Load more" moved to cursors as its first user.
- Codes: PW6912 a count beyond the read's declared most; PW6913 a cursor
  a program makes.
- Acceptance (`e14-bounded-rows`): first and next pages equal to the
  whole read's slices at two sizes, with ties and concurrent inserts; a
  cursor from another read or another user refused; a cursor carrying no
  order value (its bytes checked); the conformance suite's count.
- Mutation controls: a page that skips a tie; a cursor accepted across
  reads; `n` unbounded; descending read ascending.
- Moves: "Pagination"; "Performance budgets".

**H5 `geo-and-location`: the browser's location, a grid, distances.**
- Files: `packages/pw-platform-web/geo.pw` (`Precise`, `Cell`, `cell_of`,
  `near`, `apart`), `device.pw` (`current_location()` real, its error
  type), the runtime's Geolocation call and decoder, the host's grid
  intrinsics, `compiler/pw-core/src/device.rs` (PW6914, PW6915), tests,
  `e2e/geo.spec.mjs`.
- Codes: PW6914 a `Precise` value in a field that is not `private`, a
  log, a key or an answer; PW6915 a device permission asked outside a
  press's handler (location, camera, microphone, notifications).
- Acceptance (`e14-geo`): each rule refused beside its control; cells and
  distances against fixed points (the poles, the antimeridian, the
  equator); `near`'s cells bounded; in three engines, a press asking, an
  emulated position sent and stored as its cell alone, a refusal said on
  the page; no request carrying a coordinate but the command's.
- Mutation controls: a `Precise` stored whole; `apart` on unrounded
  centres; the permission asked on load; the decoder accepting a latitude
  past 90.
- Moves: a new row, "Location and distance" (question 7);
  "Capabilities" (a device effect's runtime).

**H6 `media`: several media a row, stripped, private, audio and video.**
- Files: `tables.rs` (`Media<K>` fields, generic blob naming), the
  uploads module (`uploads/strip.rs`, `uploads/sniff.rs`'s audio and
  video, durations, byte ranges, the session-checked route), the runtime's
  MediaRecorder behind `device.camera` and `device.microphone`, tests,
  `e2e/media.spec.mjs`.
- Codes: PW6916 a `Media<K>` field in a table with no `owner` is public,
  said (a warning); PW6917 an upload's `max_seconds` on an image kind.
- Acceptance (`e14-media`): Exif GPS absent from every stored and served
  photo (a fixture with a position, its bytes read back), PNG and WebP
  chunks and MP4 and WebM metadata the same; six photos in order and a
  reorder; a blocked or unmatched reader's request for a photo refused;
  a deleted row's blob collected; a range request answered 206 and
  played in three engines; a recording made with each engine's fake
  device, its format recorded.
- Mutation controls: stripping skipped; a blob served without the policy;
  a range ignored; a duration over the bound accepted.
- Moves: "Uploads (images)"; "Video and media" (Missing to Partial).
  The stripping lands for the feed first (Found 1).

**H7 `push`: a service worker, subscriptions, delivery.** Needs
`outbound-https`.
- Files: the build's generated worker, ADR-0333's policy (`worker-src
  'self'`), the platform's subscription table, `server/src/push.rs`
  (RFC 8291, RFC 8292, the queue), tests against a local fake push
  service, `e2e/push.spec.mjs`.
- Codes: PW6918 a notification's payload holds another principal's
  private field (preview off by default).
- Acceptance (`e14-push`): a subscription asked from a press; a message
  encrypted and decrypted against RFC 8291's example; a VAPID header
  verified; a delivery retried and then dropped; an expired endpoint
  removed; nothing sent to a muted conversation; the worker's
  notification shown in each engine Playwright can drive it in, and where
  one cannot, said.
- Mutation controls: the payload unencrypted; a reused ephemeral key; the
  mute ignored; a private text in a payload.
- Moves: "Notifications" (push); "Email, outgoing webhooks and recurring
  jobs" (a delivery queue).

**H8 `rate-limits`: a command's `rate`.**
- Files: `policy.rs` (`rate`), `codes.rs`, the host's counters in the
  platform's tables, tests.
- Codes: PW6919 `rate`'s shape, on a command alone.
- Acceptance (`e14-rate-limits`): a limit held across two hosts on one
  database; 429 with `Retry-After` and the words in the page; a limit per
  user and per session.
- Mutation controls: the counter per host; the window never reset;
  `Retry-After` wrong.
- Moves: "Production" (rate limiting).

**H9 `erasure`: delete and export a principal's data.** Needs H1.
- Files: `tables.rs` (PW6920; the erase and export plans in
  `tables.json`), the adapters' erase, the host's blob collection and
  sessions, a route answering a file (export), tests.
- Codes: PW6920 a table holding a `UserId` field with no `owner` and no
  stated reason to keep it.
- Acceptance (`e14-erasure`): after `delete_account`, no row, blob,
  session or scheduled run of the user's on memory and PostgreSQL; the
  other side's two-owner rows gone; an export holding every owned row as
  its owner sees it; a hold (if ruled) erasing at its time.
- Mutation controls: one table skipped; a blob kept; a session kept.
- Moves: a new row, "Account deletion and data export" (question 7).

**H10 `moderation`: a staff principal, reports, bans.**
- Files: the identity layer (a staff principal, a ban state refusing
  sign-in), the platform's audit log of staff reads, `privacy.rs`, tests.
- Codes: PW6921 a staff read with no audit record.
- Acceptance (`e14-moderation`): a report visible to staff alone; every
  staff read logged; a banned user's sessions ended and refused.
- Mutation controls: the audit skipped; a report shown to its subject.
- Moves: "Multi-tenancy" (roles); a new row, "Trust and safety"
  (question 7). Shares roles with the SaaS admin app.

**H11 `verification` (conditional, owner decision 5): a provider's
selfie and age checks.** Needs `outbound-https`, and a mounted resource
(ADR-0075, ADR-0339's D4) if the provider's capture is its own script.
- Files: a development verification provider beside the payment one, its
  signed answer invoking a command (`invoked_by Verification`, ADR-0332's
  pattern), the badge's rule in the app.
- Codes: PW6922 a biometric value (an image, a template, an estimate) in
  any table; the provider holds it.
- Acceptance (`e14-verification`): a signed answer badging, an unsigned
  or stale one refused; a photo changed revoking the badge; no biometric
  byte stored (the tables and blobs searched after a run).
- Mutation controls: the signature unchecked; the badge kept after a
  change; an image kept.
- Moves: "Third-party APIs".

**H12 `subscriptions`: tiers, renewals, roses.**
- Files: the development provider's subscriptions (a renewal, a lapse, a
  refund told by signed webhook), `Entitlements` written by `invoked_by
  Payments` commands, a rose spent in a serializable command, a run
  scheduled at a period's end (ADR-0334); a real provider after NEXT 6d.
- Codes: PW6923 an entitlement written by anything but a `Payments`
  command.
- Acceptance (`e14-subscriptions`): subscribe, renew, lapse at the
  period's end, refund within 14 days; two presses spending one rose
  spend it once; a lapsed subscriber's likes counted again.
- Mutation controls: the lapse never run; an entitlement written from a
  page's command; a rose spent twice.
- Moves: "Payments".

**H13 `form-bindings`: the inputs preferences are made of.**
- Files: `signals.rs` and the runtime's bindings: `bind:value` on an
  `Int` from a range or number input, `bind:checked` on a `Bool`,
  `bind:value` on a `<select>` of a closed sum type, a group of checkboxes
  on a `Set`; tests and `e2e/bindings.spec.mjs`.
- Codes: none new: the existing binding rules extended to each type.
- Acceptance (`e14-form-bindings`): each binding round-trips in three
  engines; a value outside the type refused at decode (charter §7.10); a
  binding of the wrong type refused at build.
- Mutation controls: a decode skipped; a select accepting an unknown
  case; a range read as text.
- Moves: "Forms".

### Phase 2: the app, in slices (`examples/dating/`, no Rust)

Each slice: its pages and commands, server tests on memory and
PostgreSQL, a browser suite in Chromium, Firefox and WebKit, and its
mutation script; every predicate inverted and skipped, every policy
removed, as controls. What the slices move: the rows their H tracks
moved, from "built in a reference app's Rust" (messages, notifications,
privacy by handle) to "built in a program with no Rust"; and the
reference apps' list, a dating app among them (question 7).

- **A1 `dating-profiles`** (H1, H6, H13, D3): sign-in, an adult's birth
  date, the profile with photos in order, answers, a voice and a video
  answer, vitals shown or hidden; the edit page. Acceptance: another
  user's view without hidden vitals and without the birth date, by page,
  `/pw-read`, stream and cache.
- **A2 `dating-discovery`** (H3, H4, H5): place, preferences,
  dealbreakers, the discovery queue, skip. Acceptance: a candidate who
  fails either side's dealbreakers never shown; a paused or blocked
  candidate never shown; the distance the cells' band; the 2024 oracle
  run as a test (a reader moving in steps learns nothing finer than a
  cell).
- **A3 `dating-likes`** (H2, H3): a like on a part with a comment, Likes
  You (one at a time, all for a subscriber), roses, standouts, a match on
  a mutual like, the comment filter. Acceptance: two simultaneous likes
  make one match on PostgreSQL; a free user's view holds one like and a
  count, in every answer.
- **A4 `dating-chat`** (H2, H4): matches, the conversation by cursor,
  Match Note, Your Turn Limits, unmatch, We Met. Acceptance: a third user
  sees nothing of a conversation; after an unmatch, both sides' pages
  told and every read empty.
- **A5 `dating-safety`** (H1 to H3, H9; H10 for review): block, report,
  pause, delete account. Acceptance: a block hides each from the other by
  every path, media included; a report blocks both ways; a deletion
  leaves nothing.
- **A6 `dating-notify`** (H7): in-app notifications of likes, matches and
  messages, push, per-conversation mute.
- **A7 `dating-paid`** (H12): Hinge+-like and HingeX-like tiers and roses
  on the development provider.

## Downloads for the owner's approval

Read from crates.io's API, ffmpeg.org and download.geonames.org on
2026-10-25; resolved by hand from crates.io's dependency metadata (a
`cargo generate-lockfile` at approval is the record); nothing fetched.

**1. Outbound TLS, shared with NEXT 6d's Stripe adapter, H7 and H11.**
`rustls` with the `ring` provider and the operating system's roots. New
names on Linux, 2,102,460 bytes:

| Crate | Version | Published | Licence | Bytes |
| --- | --- | --- | --- | --- |
| `rustls` | 0.23.46 | 2026-10-14 | Apache-2.0 OR ISC OR MIT | 386,227 |
| `ring` | 0.17.14 | 2025-03-11 | Apache-2.0 AND ISC | 1,502,610 |
| `rustls-webpki` | 0.103.16 | 2026-10-20 | ISC | 89,984 |
| `rustls-pki-types` | 1.15.1 | 2026-07-23 | MIT OR Apache-2.0 | 39,322 |
| `rustls-native-certs` | 0.8.5 | 2026-10-13 | Apache-2.0 OR ISC OR MIT | 21,110 |
| `openssl-probe` | 0.2.1 | 2026-01-20 | MIT OR Apache-2.0 | 9,155 |
| `untrusted` | 0.9.0 | 2021-07-13 | ISC | 14,447 |
| `subtle` | 2.6.1 | 2024-06-24 | BSD-3-Clause | 14,562 |
| `zeroize` | 1.9.1 | 2026-10-06 | Apache-2.0 OR MIT | 25,043 |

- For other targets in the lockfile: macOS's `security-framework` 3.7.0,
  `security-framework-sys` 2.17.0, `core-foundation` 0.10.1 and
  `core-foundation-sys` 0.8.7 (176,791 bytes, MIT OR Apache-2.0);
  Windows's `schannel` 0.1.30 (MIT, 34,744 bytes, on the lock's
  `windows-sys` 0.61); and, for `ring` on aarch64 Windows alone,
  `windows-sys` 0.52.0 with `windows-targets` 0.52.6 and its eight target
  crates (MIT OR Apache-2.0, 8,213,752 bytes). Already in the lock: `cc`,
  `cfg-if`, `getrandom` 0.2, `libc`, `log`, `once_cell`, `rustversion`.
- **ISC must be admitted to `deny.toml`** (`ring`, `rustls-webpki` and
  `untrusted` have no other licence): a permissive licence, and a
  decision `deny.toml`'s comment says is recorded in
  `docs/research/technology-matrix.md` as well.
- **Not proposed**: `webpki-roots` 1.0.11 (CDLA-Permissive-2.0, not
  admitted; the operating system's roots instead) and `aws-lc-rs`,
  rustls's default provider (a C and assembly build).
- `rustls` 0.23.46, `rustls-webpki` 0.103.16 and `rustls-native-certs`
  0.8.5 are under two weeks old at this date; an approval may take the
  newest patch older than that.
- **Push needs nothing more**: `ring` 0.17.14 provides `ECDH_P256`,
  `ECDSA_P256_SHA256_FIXED_SIGNING`, `hkdf` and `AES_128_GCM` (docs.rs,
  read 2026-10-25), all RFC 8291 and RFC 8292 use.

**2. Optional, thumbnails (H6, owner decision 4)**: `image` 0.25.10
(2026-03-10, MIT OR Apache-2.0, 303,647 bytes) with default features off
and `png`, `jpeg` and `webp`: 14 new names, 1,947,010 bytes (`image`,
`bytemuck` 1.25.2, `byteorder-lite` 0.1.0, `moxcms` 0.8.1, `pxfm` 0.1.30,
`num-traits` 0.2.19, `zune-jpeg` 0.5.15, `zune-core` 0.5.3, `png` 0.18.1,
`fdeflate` 0.3.7, `simd-adler32` 0.3.10, `flate2` 1.1.10, `image-webp`
0.2.4, `quick-error` 2.0.1), and `miniz_oxide` 0.8.9 beside the lock's
0.9.1 (67,132 bytes); every licence one `deny.toml` admits. Not needed
for stripping, which H6 writes itself.

**3. Optional, transcoding and poster frames (H6, owner decision 4)**:
FFmpeg 9.0.2 "Lei" (2026-09-18), `https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz`,
12,040,788 bytes, its signature 520 bytes; LGPL-2.1-or-later, GPL if built
with its GPL parts (ffmpeg.org, Legal). A native program the host would run
on untrusted media, so a sandbox of its own; building it needs a C
toolchain and an assembler not listed here. Not recommended for the first
slices.

**4. Optional, place names (H5, owner decision 3)**: GeoNames
`cities15000.zip` (3,363,204 bytes, about 25,000 places) or
`cities1000.zip` (11,080,467 bytes, about 130,000), both last modified
2026-10-24 01:56:31 GMT and regenerated daily, so an approval pins a
SHA-256 at download; "licensed under a Creative Commons Attribution 4.0
License" (its readme), credited on the app's page. Hinge's names come from
Google Maps *(help, excerpt)*: not proposed.

**Not needed**: H3 hexagons (`h3o` 0.11.0, BSD-3-Clause, 2,844,408 bytes):
a square grid of our own does. Playwright's geolocation and notification
permissions are built in; fake camera and microphone streams are engine
switches, measured, not downloaded.

## Owner decisions

1. **The downloads**: 1 (TLS, with ISC admitted), and whether 2, 3 or 4.
2. **This app among the reference apps**, and where in the owner's order
   (NEXT): it covers location, mutual-consent privacy, trust and safety,
   media and push, which no planned app does.
3. **Location**: the grid's cell size (the coarsest a dater accepts; a
   finer cell is a finer oracle), and whether others see a place's name
   (download 4, CC BY) or a distance band alone. Precise coordinates are
   never stored, as proposed.
4. **Media**: voice and video kept as each browser records them, an
   engine that cannot play one said so; or FFmpeg (download 3).
   Thumbnails by `image` (download 2) or none.
5. **Verification and age assurance**: a provider (its biometric data,
   its jurisdictions, its JavaScript in our pages, a mounted resource), or
   neither in this app, said.
6. **Payments**: tiers and roses on the development provider first; a
   real provider after NEXT 6d's crates.
7. **Moderation**: a moderator's page in this app, or in the SaaS admin
   app; automated text or image screening providers: none proposed.
8. **Push**: approve with download 1 and a VAPID key per deployment (a
   deployment secret, which nothing manages yet), or in-app only;
   previews on a lock screen off by default.
9. **Retention after deletion**: erased at once, or a safety hold (Hinge
   keeps three months), a deployment's legal call.
10. **The free tier's numbers** (likes a day, roses a week, the turn
    limit): ours, since Hinge does not publish its.

## Questions for the integrator

1. **Codes**: PW6901 to PW6923 (PW69 free on all 162 origin refs,
   2026-10-25; PW67 is ADR-0339's reservation, PW68 `DeviceStore`'s); or
   the table clauses' refusals (PW6901 to PW6904, PW6909 to PW6913,
   PW6920) in `DeclarationRules`' PW03xx, as ADR-0338's ruling 4 placed
   D1's?
2. **ADR-0115**: decision 2 lets a program give a predicate a body. It
   changes a rule other code relies on (`identity.rs`'s `holds`, the
   exports' `authorization`, ADR-0307's asking). Amend?
3. **ADR-0279**: two owners read from each side keep "No label names two
   principals". Confirm.
4. **ADR-0129**: a private field's write becomes a sink (PW6903). Confirm
   the scope: private fields of owned tables alone.
5. **Files shared with D1 to D3**: H1, H3, H4 and H9 change `tables.rs`,
   `pw-table-memory` (D2's, drafts #29 to #40 open) and D3's adapter.
   After D2's writes land, or as D2's follow-ups?
6. **`outbound-https`**: one platform track for NEXT 6d, H7 and H11? Its
   home: the host's `network.fetch`, or the server?
7. **The capability matrix** (the integrator's file): new rows "Location
   and distance", "Mutual-consent privacy", "Trust and safety: blocking,
   reporting, moderation" and "Account deletion and data export"; and a
   dating app among the reference apps.
8. **The museum** (ADR-0342): the 2024 paper's three Hinge findings (the
   distance filter's oracle, a like in the card stack, a like's time) as
   exhibits, each shown happening in Next.js or SvelteKit by its protocol?
9. **The app's home**: `examples/dating/`, the fifth app, with no Rust and
   no migrations of its own.
10. **Ports** for the browser specs of H5 to H7 and A1 to A7.

## Not claimed

- **Hinge's recommendations**: no learned model, Top Photo, Prompt
  Feedback or Convo Starters; the rank is a declared function. A model
  would be a provider behind `network.fetch`.
- **Screenshot protection**: no browser can prevent a screenshot.
- **A Block List from the device's contacts**, and blocking by phone or
  email: accounts verify neither.
- **Native apps**; push on iOS only for Home Screen web apps.
- **Resistance finer than a cell**: a cell is shown by design; mutual
  dealbreakers disclose a bit a candidate, declared and rate-limited.
- **Fraud, fake and duplicate accounts** beyond the identity provider;
  background checks (Hinge makes none either).
- **Detecting exchanged phone numbers** (We Met's trigger in Hinge) or
  scanning messages ("Are You Sure?") past a word list.
- **Legal compliance** (age-assurance laws, data protection): the
  deployment's; this note gives it mechanisms.
- **End-to-end encryption of chats**: NEXT's later app.
- **Scale**: discovery computed on one host per request; several hosts
  through ADR-0369's shared database only.
- **A paywall held as privacy**: a like is its recipient's to read
  (`shares to`), so the free tier's "one at a time" is the app's rule in
  its query; another query of the same app could show more. The checker
  holds whose data reaches whom, not a business gate (the paper's
  card-stack leak was both; Pleris answers its privacy half).
- **A message deleted alone**: either owner of a conversation may delete
  its rows, and an unmatch deletes them all for both; who may delete one
  message (its author alone) is not designed.
- **Timing**: what a predicate's or a policy's cost tells a reader.
- **i18n**, and accessibility past the suites the project already runs.

## Found while researching

Corrections to the record first, then findings.

1. **The capability matrix is stale on D2**: it says `track/tables-d2` is
   uncommitted; master has `pw-table-memory` (PR #12, merged 2026-10-24)
   and draft PRs #29 to #40. The integrator's file.
2. **ADR-0339's ruling 3 is unbuilt**: `requires` on a query, accepted
   2026-10-15; at `cc47a64b`, `check.rs` refuses `requires` off a command.
3. **The feed serves its users' photo locations today.** ADR-0260's not
   claimed: "an Exif GPS position is served as uploaded". Any feed image
   from a phone that records its place publishes where it was taken, to
   anyone, at an immutable address. Not a dating-app issue: a reference
   app's live privacy gap. H6's stripping should land for the feed first.
4. **Two location types, neither with a unit**: `device.Location { lat:
   Int, lng: Int }` (`packages/pw-platform-web/device.pw`) and `LatLng` of
   `Float`s (`examples/domain.pw`, which `examples/lib/Maps.pw` uses); and
   `placement.rs`'s test vocabulary names `device.geolocation` where the
   platform declares `device.location`.
5. **Hinge's location bug was its filter, not its display** (the 2024
   paper): hiding a number fixes nothing while a comparison uses the
   precise value. What a framework must hold is what a comparison uses.
6. **This PC's pnpm store lacks master's `zod` 4.6.7** (the override of
   656d47ae and c0852753), so a fresh worktree cannot install the
   workspace offline, and `just ci`'s evidence gates then fail six tests
   for want of `@playwright/test`. An offline install of the one project
   they need (`pnpm install --frozen-lockfile --offline --filter
   pw-own-renderer-spike`) links it from the store, with no download.
7. **help.hinge.co refuses automated reads** (HTTP 403): its facts here
   are search excerpts, to be read again by a person.
8. **No platform function gives a program its reader's own id**: the feed
   reads it through Rust (`author(current_session())`), which a program
   with no Rust cannot (decision 1's `id_of`).
