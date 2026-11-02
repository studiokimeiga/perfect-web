# ADR-0260: an image on a post, a typed upload served safely

Status: accepted at its merge, 2026-10-07, under the owner's delegation of
2026-10-02; proposed by track `uploads` (W2, `track/uploads`), under the
integrator's rulings of 2026-10-07 on its four questions and its one ask
(below, "The integrator's answers"), and its questions at the merge
answered after them. Date: 2026-10-07. Milestone: E14, the owner's Twitter
list, item 7.

## Context

- **A post was text.** The owner's list asks that a post in the feed carry
  an image. Pleris builds no storage and no CDN (the charter): a deployment
  supplies blob storage, and Pleris decides what reaches it and how it is
  served.
- **The seams were there** (ADR-0253): `Uploads::claims(method, route)`,
  asked before a request's body is read, and `Uploads::answer` for one it
  claims, so an upload reads its own body within its own limits where a
  command's is bounded to 64 KiB.
- **The language holds an acquisition** (ADR-0250, ADR-0251): a value whose
  row says `resource.acquire<T>` is held by a name, ended where it is made,
  returned, or held by a resource's `acquire` clause, and a
  `resource.release<T>` ends it exactly once on every path (PW2005).
- **What a browser shows an image at is decided before it loads only when
  the markup says it** (ADR-0187): `<img width height>` gives the box its
  aspect ratio.
- **The identity track merged as this track ended** (ADR-0258): a
  session's principal, `requires SignedIn`, and `delete`. The track rebased
  onto it and takes its principals (§3a).

## Research

Each read 2026-10-07.

- **What a file is, from its bytes.** WHATWG MIME Sniffing, §6.1 "Matching
  an image type pattern": `GIF87a`, `GIF89a`; `RIFF`, four bytes, `WEBPVP`;
  `89 50 4E 47 0D 0A 1A 0A`; `FF D8 FF`.
- **PNG** (W3C PNG, third edition): "The IHDR chunk shall be the first
  chunk"; width and height four-byte unsigned integers, "Zero is an invalid
  value", at most 2³¹−1; each chunk's CRC over its type and data.
- **JPEG** (ITU-T T.81, Annex B): SOFn is `C0`–`C3`, `C5`–`C7`, `C9`–`CB`,
  `CD`–`CF`; a marker may be preceded by fill bytes `FF`; a frame header's
  line count `Y` may be zero, defined later by DNL, and its `X` not.
- **WebP** (RFC 9649): the RIFF size counts from offset 8; `VP8 ` per RFC
  6386 §9.1 (a key frame's tag, start code `9D 01 2A`, 14 bits of size and
  2 of scale, little-endian); `VP8L` (signature `2F`, 14 bits of width − 1
  and of height − 1); `VP8X` (24-bit canvas width − 1 and height − 1, whose
  product "MUST be at most 2³² − 1").
- **GIF** (GIF89a, §17, §18): the logical screen's width and height,
  "Least Significant Byte first".
- **A JPEG turned by Exif.** TIFF 6.0 (the structure Exif's APP1 carries):
  `II`/`MM`, 42, the first IFD's offset; 12-byte entries; `Orientation`,
  tag 274 (`112.H`), SHORT, where 5 to 8 make "the 0th row … the visual
  left-hand side" or right: rows and columns exchanged. CSS Images 3:
  `image-orientation`'s initial value is `from-image`, and "The natural
  height and width are derived from the rotated rather than the original
  image dimensions". CIPA DC-008 itself sits behind a disclaimer page that
  no fetch here could pass; TIFF 6.0 is the primary source used, and the
  browser suite checks what three engines decode.
- **A form's file.** RFC 7578 §4.2: a part that is a file's content has a
  `filename`; RFC 2046 §5.1.1: a boundary is 1 to 70 characters, and "The
  boundary delimiter MUST occur at the beginning of a line".
- **Serving.** RFC 8246: `immutable` "indicates that the origin server will
  not update the representation"; a content-addressed key never names other
  bytes. `X-Content-Type-Options: nosniff` and `Content-Security-Policy:
  sandbox` for a document a browser is navigated to.
- **Cross-origin requests**: Go 1.25's `CrossOriginProtection`, as the
  identity track read it (`Sec-Fetch-Site`, then `Origin` against `Host`).
- **sha2 0.11.0** is already in `Cargo.lock` (through postgres-protocol),
  with its default features' dependencies: nothing downloaded.
  `Sha256::digest` per its README and source; FIPS 180-4's "abc" vector is
  a test.

## Decision

### 1. The program states its upload, and the host holds a browser to it

```text
upload PostImage
    route      "/uploads/post-image"
    serves     "/images"
    max_bytes  5_000_000
    types      png, jpeg, webp, gif
    max_width  4096
    max_height 4096
```

- **A resource noun, parsed and checked like `source`** (the integrator's
  Q1 ruling): `DeclKind::Upload`; its heads `serves`, `max_bytes`, `types`,
  `max_width`, `max_height` (and `route`, a page's or an upload's) in the
  clause table, each held to its domain (PW0335) and to its place (PW5105).
  `types` is a closed set (`policy::UPLOAD_TYPES`); the limits are a new
  domain, `Count`: digits, `_` between them, greater than zero.
- **PW5601**: each clause stated; its paths literal (`/` and lowercase
  segments, no parameter); `max_bytes` at most 100,000,000, since a
  development host reads a form whole; a width or height within 2³²−1.
- **PW5602**: what an upload answers is its own: its route, the lease shown
  under it (`<route>/{lease}`), and what it serves (`<serves>/{key}`),
  matched against every page's route and every other upload's.
- **PW5603**: a form that sends a file posts it, `method="post"` and
  `enctype="multipart/form-data"`, to a declared upload's route, one file
  input in it; and a form posting to an upload's route is such a form. A
  form is checked as PW5009 checks a link: against what the program
  declares. Forms that send no file are not checked (the identity track's
  `/sign-in` forms among them).
- **`pw build` writes `uploads.json`** where the program declares one; the
  host reads it as it reads `sources.json`. An upload's route is not a
  page: `routes::table` leaves it out, so a link to it is PW5009's.
- **A deployment may lower a limit and never raise one**
  (`PW_UPLOAD_MAX_BYTES`, `..._WIDTH`, `..._HEIGHT`): a value above the
  program's refuses the server's start, which says which, rather than being
  taken as the lesser. The program's statement wins; a deployment narrows it.

### 2. What a file is, read from its bytes

`uploads/sniff.rs`: the kind by its signature alone, then its header, each
reader the format's own: PNG's IHDR (its CRC checked), JPEG's first SOFn
(DNL's zero refused, an Exif `Orientation` of 5 to 8 exchanging width and
height as a browser shows it), WebP's first chunk (`VP8 `, `VP8L` and its
version, `VP8X` and its product), GIF's logical screen. The `Content-Type`
a browser sent and the file's name are never read. An unknown signature is
415; a known one whose header is not whole, 422; a kind the declaration does
not list, 415; past its width or height, 422; past its bytes, 413.

### 3. A form's body read within its limits

`Uploads::answer` (`main.rs`'s seam now passes the method):

- a session the browser was not given is 403; a request from another
  origin, 403; one without a `Content-Length`, or chunked, 411;
- a length past `max_bytes` and 16 KiB of form is 413 before a byte is read,
  and what follows is drained, bounded, so the browser reads the answer;
- the body is `multipart/form-data`, parsed by RFC 2046's delimiters; the
  file is the one part with a `filename`, whatever its field is called;
- refused, the browser is shown why, in a page that runs nothing, with the
  way back (the `Referer`'s path where it is this origin, `/` otherwise);
  leased, it is sent back there (303).

### 3a. Who uploads, and how much

- **A signed-in user** (track `identity`'s principals, handed to the
  uploads once as a data layer is handed them): one signed out, in the
  accounts model, attaches nothing (403), since `requires SignedIn` holds
  its post anyway; in the guest model every session is its own guest. The
  home page shows the attach form to one signed in.
- **A budget per user**: 30 images an hour, and 20 times the declaration's
  bytes an hour, whichever is spent first, each upload spent whether posted
  or not, since what it costs is the server's work and memory. Past it,
  429 with `Retry-After`, the seconds until the hour's oldest is spent.
- **The same-origin check is identity's** (`identity::same_origin`): the
  upload route is answered before `Identity::answer`, so it asks it itself.

### 4. A lease, claimed by a command, committed with a post or discarded

- **The bytes wait in the server's memory**, by their SHA-256, leased to the
  session: one lease a session, which a second upload replaces; a lease
  nothing claims ends after an hour; all sessions' leases hold at most 128
  MiB together. No lease outlives its server, and nor do its bytes.
- **The program claims and releases it** (Q2): in `examples/feed/app.pw`,
  ```text
  command post_image(text: PostText, alt: AltText) -> Result<Post, FeedError>
      ..
  {
      let image = claim(current_session())?
      publish_image(current_session(), text, alt, image)
  }
  ```
  `claim` is `resource.acquire<Upload>`; `publish_image` and `discard` are
  `resource.release<Upload>`, so PW2005 refuses a body that leaks it
  (checked: `publish(..)` in its place is "affine resource `image: Upload`
  is not consumed on every path"). The handle never reaches the page or a
  signal: it lives in the body.
- **The host holds a claim to its command's transaction** (`uploads::Claims`):
  claimed, no other command may claim it and no upload or sweep ends it;
  committed, the post's image is put in the deployment's blob storage
  *before* the transaction commits, and the lease ended after; discarded,
  the lease ends with the commit; a command that refuses, fails or traps
  drops its claims unsettled, and each is given back. A handle this command
  did not claim is not found.
- **The image is the post's own data** (Q3): a field of `feed.rs`'s `Row`,
  and five columns of `posts` on PostgreSQL (migration `0005_post_images`,
  all five or none by a `CHECK`), written in the post's transaction.
- **The layers are handed the leases once**, `DataLayer::uploaded_by`,
  defaulted to nothing (the integrator's ruling on the ask, as
  `identified_by` is); `FeedPg` keeps them in a `OnceLock`.

### 5. Served safely

- **Only what a post committed is served**: from the deployment's blob
  storage (`BlobStore`; `LocalBlobs`, a directory, for development:
  `PW_BLOB_DIR`, or `blobs/` beside the build), at
  `<serves>/<64 hex digits>.<kind>`. A key is those digits or nothing, so no
  path is built from anything a sender wrote. Its type is sniffed from its
  bytes again as it is served, and a path whose extension is not its kind
  is 404.
- **Its headers**: the sniffed `Content-Type`; `X-Content-Type-Options:
  nosniff`; `Content-Security-Policy: default-src 'none'; sandbox`, for
  anyone navigated to it; `Cache-Control: public, max-age=31536000,
  immutable`; an `ETag` of its key, `If-None-Match` answered 304;
  `Content-Disposition: inline` with the key for a name;
  `Cross-Origin-Resource-Policy: same-origin`.
- **A lease is shown to its session alone**, `private, no-store`, under the
  upload's route, so its author sees what they attached before posting.
- **SVG is not an image an upload may be**: `types` lists none that runs.

### 6. The feed

- `type Image = Image { src, width, height, alt }`, `images: List<Image>`
  on `Post` and `Item`, none or one (see Found), and `opaque type AltText`,
  1 to 1000 characters, which `post_image` requires.
- The home page: the attach form (a plain multipart form, no runtime
  change); the attached image shown with its size and a Remove button
  (`discard_image`); a field for the image's words; Post sends `post_image`
  where one is attached and `post` where none is.
- Each timeline row, and a thread's every post, shows its image as `<img
  class="photo" src width height alt>`.

## Found

- **A row's record past 16 flat values does not build.** `image:
  Option<Image>` on `Item` made Home's and the following page's derived
  values take 18 (the Canonical ABI's limit is 16), and the wasm backend
  refuses indirect parameters (`backend/wasm.rs`, "an export whose
  parameters exceed the flat limit"). `images: List<Image>` is 2, and
  Twitter's own model. The integrator queues indirect parameters.
- **A host binding not of the form `package:ns/interface#name`**
  (`feed:uploads#claim`) broke WIT generation for every component, with an
  error naming no source line. The integrator queues a refusal where it is
  written.
- **A handler takes no `Option` apart** (ADR-0033): the home page's submit
  handler reading `Option<Attached>` was refused at serving ("`none` cannot
  be captured by a handler"). The query is `List<Attached>`, none or one.
- **A signal bound inside a `{#match}` arm is refused** (ADR-0137): the
  image's words are a field of the post form, always shown.
- **Every build writing `uploads.json`** made the committed kiokun build
  stale (`evidence_is_current`); a program that declares none has none.
- **Two servers of one build shared a staged directory**, each emptying it
  at its start: leases moved to memory.
- **The browser suite's first full run** found the attach form's label
  ("An image, one a post") matched `getByRole("button", { name: "Post" })`
  in `feed.spec.mjs`; it says "An image to attach", and that suite's one
  `locator("form")` finds the post form by its draft.
- **An intermittent**: the first `just e14-uploads` here failed
  `on_postgres_an_image_is_kept_with_its_post` once, and recorded only its
  name. Fifteen runs of the same command since, and the suite's runs before,
  have not failed it. The recipe now keeps a failure's panic. What it was
  is not known; a guess is a query's 2-second timeout under the machine's
  load, which every feed test shares.
- **Two more, on CI** (verify run 37710494244, at b98fb32): the push
  planned 24 recipes, and two failed in a mutation script's unmutated
  baseline, after the same recipe's own run of the same tests passed:
  `e14-not-found` (the server's 249 passed, then its baseline 248 of 249)
  and `e14-computed-rows` (the feed's browser suite 60 of 60, then its
  baseline 50 of 51). Neither script names the test its baseline failed,
  so which is not known. Not re-run; each is the integrator's to judge.
  `e14-uploads` and `e14-identity` passed there.

## Alternatives

- **Limits in the deployment alone.** No compiler change, and a program
  that states nothing of what it accepts: refused by the integrator (Q1).
- **A file input bound in the language** (`bind:file`): a runtime and
  compiler change, and the bytes in the page's state. A host form reaches
  the same lease, and the handle stays in the command's body.
- **A wrapper data layer decorating posts with images**: no change to the
  feed's layers, and an image kept apart from its post, lost on PostgreSQL
  at restart. Refused (Q3).
- **Serving a lease where committed images are**: unguessable by its hash,
  and still the bytes of a post no one made. Not served.
- **Staged bytes on disk**: survived a crash with no lease to end them, and
  two servers of one build forgot each other's.

## Consequences

- **A deployment supplies `BlobStore`**: `put` by the bytes' key, `get`
  exactly those bytes, `delete`, `count`. A CDN in front of `serves` may
  cache forever.
- **The feed's `Item` and `Post` carry `images`**: every constructor in
  app.pw writes it, and the identity track's `Item.mine` merges beside it
  in field order.
- **The browser posts the file before the text**: attaching navigates (a
  plain form); a draft typed before attaching is lost. A runtime that sends
  a file would keep it (Not claimed).

## Acceptance

Recorded by `just e14-uploads` in `docs/evidence/E14/uploads.txt`, at
2af787b, PostgreSQL 18.6 named locally:

- **The server, 47 tests**, `uploads`, `uploads/sniff.rs`,
  `uploads/multipart.rs`, `blob.rs` and `tests/uploads.rs`: the header readers against
  hand-written headers and against real encoders' files
  (`scripts/uploads_fixtures.py`, Pillow, committed under
  `e2e/uploads/`: PNG, baseline, progressive and Exif-turned JPEG, lossy,
  lossless and extended WebP, GIF); the form's parts; keys and both blob
  stores; routes claimed; a lease to its session; sniffing over the
  sender's word; size before and after reading; width and height; session,
  length and origin; the way back; a second upload; a lease's time; a claim
  once; a deployment's limits; a claim given back; a user's hourly budget,
  by count and by bytes; one signed out refused; and over HTTP through
  `main.rs`'s seam, in memory and on PostgreSQL: attached, posted, shown to
  every reader with its size and words, in a timeline and a thread, served
  with its headers; none attached commits nothing; removed, forgotten; a
  command that does not commit gives its claim back; an upload read past a
  command's 64 KiB and refused past its 5 MB; what is not an image refused.
- **The declaration** (`compiler/pw-core/tests/uploads.rs`, 10): each
  clause stated, each limit a literal count, its types closed, its paths
  literal and its own, its clauses its own, and forms posting a file.
- **The browser, in Chromium, Firefox and WebKit** (`e2e/uploads.spec.mjs`,
  5 tests, 15 of 15): see its header; among them, the natural size each engine
  decodes for each of 8 files equals its markup, the Exif-turned JPEG's
  included, and an image held back already has its box.
- **`scripts/uploads_mutations.py`: 26 mutants, 26 killed**, among them
  the kind taken from the sender, the size limit raised, a key that is not
  hex digits so a path built from what a sender wrote, `nosniff` dropped,
  one signed out attaching, a user's budget not held, and an image's width
  and height left out of the markup (killed in the browser). Its first run
  here left one: PW5601's limit was a diagnostic, and nothing held
  `upload_clauses` to leaving such an upload out of `uploads.json`; a test
  does now.
- **The whole workspace's tests** (`cargo test --workspace`, PostgreSQL's
  among them); the whole browser suite, 812 passed and 13 skipped as on
  master; `just lint`, `cargo fmt --check`, `just evidence-gates`, `just
  case-check`, `just mutation-anchors` (1840 mutants in 201 scripts) and
  `just audit`, each green at the rebased head.

## Not claimed

- **A budget kept across restarts or servers**: the hour's spending is the
  server's memory, as its leases are; a deployment of several hosts would
  keep it in the database.
- **Deleting a committed image.** A post deleted (identity's `delete`,
  merged) leaves its blob; blobs are content-addressed and may be shared
  by posts, so collecting them is a count of the posts that name each key.
- **A production blob store or CDN**, signed URLs, private images, image
  re-encoding or resizing, stripping metadata (an Exif GPS position is
  served as uploaded), animated images' frames checked beyond GIF's screen
  and WebP's canvas, AVIF, and decompression bombs beyond the declared width
  and height.
- **EXIF orientation outside JPEG** (a PNG's `eXIf`, a WebP's `EXIF`
  chunk): read as stored.
- **Attaching without a navigation**, more than one image a post, and an
  optimistic post with its image.
- **`e14-uploads` on CI's database job**: CI plans it where browsers are;
  its PostgreSQL tests pass doing nothing there, and ran here.

## Questions for the integrator

1. **Collecting committed blobs.** When identity's `delete` merges, should
   the uploads track count posts per key (both layers) and delete a blob no
   post names, in the deleting transaction's commit hook as `keep` is in
   the posting one's?
2. **A runtime that sends a file** (`<input type="file">` posted by the
   page, the lease's id never shown to it), so a draft survives attaching:
   a runtime change, queued or not?
3. **`e14-uploads` and the database job**: add it to `NEEDS_DATABASE`
   where browsers are installed, as identity asked for `e14-identity`.
4. **Form checks beyond file forms**: PW5603 checks forms that send a file.
   A general check of every form's `action` against what the program and
   its host declare is W1's Q2 too.

## The integrator's answers

- **Q1, limits declared by the program: yes.** An `upload` declaration,
  parsed and checked like `source`, written by `pw build` to
  `uploads.json`, read by the host as `sources.json` is. Its limits
  literals, `types` a closed set, its route no page's (a PW56xx code), and a
  deployment may lower a limit and never raise one. A form posting to it
  checked as PW5009 checks a link, if cheap.
- **Q2, affine leases: yes.** `claim` acquires, publish or discard release,
  in the command's body; `resource.acquire<Upload>` and
  `resource.release<Upload>` granted on the development origin; the handle
  kept out of signals and the browser.
- **Q3, data layer: no wrapper.** The image is the post's own data: a field
  of `feed.rs`'s `Row`, columns of `posts` by migration 0005, written in the
  post's transaction; blobs the uploads module's. Started after ADR-0257.
- **Q4, app.pw**: the image on `Post` and `Item`, a required `AltText`,
  `<img>` with width and height, additions marked, every constructor given
  its image.
- **The ask: approved.** `DataLayer::uploaded_by`, defaulted, called once
  in `from_build_with` at a marked seam, kept in a `OnceLock`;
  `from_build_with`'s signature kept. A third such hand-off would merge
  them into one struct.
- **The flat-limit finding: accepted**, `images: List<Image>`. The
  integrator queues indirect parameters past 16 flat values, and a refusal
  of a malformed host binding where it is written. The image's words always
  shown: fine.

## The integrator's answers at the merge

2026-10-07.

1. **A deleted post's image: stop serving it at once, and collect its
   blob.** Delete promises its author the post is gone, and its image,
   served at a key anyone shown it can keep, was not. Serving asks the
   layer whether a committed post names the key, both layers by an index,
   and answers 404 where none does; a blob no post names is deleted after
   the deleting transaction commits, unless a command in flight holds a
   claim to the same key, which will name it or give it back and be
   collected then. A copy a CDN or a browser keeps under `immutable` is the
   deployment's to purge as its store deletes. Queued first in NEXT; built
   by ADR-0261.
2. **A runtime that sends a file: yes, queued** after notifications. The
   page posts the file itself and reads the attached image as the attach
   form's answer shows it, so a draft typed before attaching survives; the
   plain form stays for a page without its runtime.
3. **`e14-uploads` on the database job: yes, at this merge.**
   `NEEDS_DATABASE` names it, and the database job sets up the browsers
   and the build for it (ADR-0258's merge).
4. **Every form's `action`: yes, with the host's routes** (ADR-0258's
   second answer). The platform states the routes a deployment serves, the
   identity's and an upload's, and every form's `action` and `method` is
   checked against them and the program's pages and commands, as PW5009
   checks a link; PW5603 stays a file form's own check. One item in NEXT.

**The intermittents.** `e14-not-found`'s baseline failing one server test
of 249 is most likely the estimate test that read its region one way it can
arrive, fixed at `e447b72` (STATUS's correction of 2026-10-07): a recipe
keeps a failure's `panicked at` line since, and a feed test's failure what
the network saw (`1fcabdd`), so the next of either says which.
`e14-computed-rows`' browser baseline and this track's one local
`on_postgres_an_image_is_kept_with_its_post` stay unknown until then.

## At the merge

- Numbered ADR-0260; `ADR-XXXX` reads `ADR-0260` in the code, the recipe
  and the scripts. The recorded evidence keeps the header it was recorded
  with, at `2af787b`, before the track's last rebase; the merge's
  verification records `e14-uploads` again, on the database job.
- `scripts/ci_plan.py` plans `e14-uploads` on the database job.
- `docs/PARALLEL.md`: W2 merged; W3, notifications, added under the
  integrator's rulings.

## Report

**Built.** The `upload` declaration and PW5601–PW5603 (`pw-syntax`'s
keywords; `pw-core`'s `uploads.rs`, `policy.rs`'s `Count` domain and
clause table, `build.rs`'s `uploads.json`); the server's `uploads.rs`,
`uploads/sniff.rs`, `uploads/multipart.rs` and `blob.rs`; the feed's image
in both layers and migration 0005; the feed's pages; the browser suite;
the fixtures and their script; the mutation controls; `just e14-uploads`.

**Storage and serving.** Leases in the server's memory, by SHA-256, one a
session, claimed by a command's transaction and ended with it; a post's
image put in the deployment's `BlobStore` before its transaction commits;
served from it alone at `/images/<sha256>.<kind>`, typed by its bytes,
`nosniff`, sandboxed, immutable.

**Dependencies.** None new: sha2 0.11.0 was in `Cargo.lock`. Node's
packages were installed from the local pnpm store, offline, at the lock's
versions.

**Tests and mutants.** The server's 47, the compiler's 10, the browser's
15 of 15 in three engines, 26 of 26 mutants killed
(`docs/evidence/E14/uploads.txt`); the workspace, the whole browser suite
and the CI gates green.

**Merge notes.**
- Commits, in order: the server's upload, sniffing, leases and serving;
  the declaration; `uploads.json` only where declared; the feed's image; a
  form's file by its filename; leases in memory; the server's start line;
  the browser suite; the mutation controls and the recipe; the recipe
  keeping a failure's panic; a malformed upload not written; who uploads
  and their budget; this ADR and its evidence.
- Shared files: `Cargo.toml` and the server's (`sha2`, the line identity
  adds too: keep one); `data.rs` (`uploaded_by`); `feed.rs`, `feed_pg.rs`,
  `migrations/feed/0005_post_images.sql`; `examples/feed/app.pw` (marked);
  `main.rs` at lines marked `TRACK SEAM (uploads)`: `mod blob`, the seam's
  method, building the uploads, `uploaded_by`, the start line, the tests'
  module; `playwright.config.mjs` (`UPLOADS_PORTS`, PORT+80..82);
  `feed.spec.mjs` (one locator); `computed_rows_mutations.py` (one
  anchor); the compiler: `grammar.rs`, `hir.rs`,
  `lower.rs`, `rules.rs`, `check.rs` (one call, one message), `koka.rs`,
  `resolve.rs`, `policy.rs`, `routes.rs` (`table`), `build.rs`, `codes.rs`
  (PW5601–PW5603, `Owner::Uploads`), `lib.rs`, `pw-cli`'s listing.
- Rebased onto identity's merge (32f293b): `Item` is `.., likes, mine,
  images`, each layer writing them in that order; `publish-image` writes as
  the author `publish` does (identity's `author`); the uploads take the
  identity's principals beside the layers' (`Uploads::identified_by`, at
  the seam that builds them); the upload route asks identity's
  `same_origin`; the attach form is inside the home page's `{#if
  me.signed_in}`; `computed_rows_mutations.py`'s anchor on `liked` carries
  `mine` and `images`.
- New environment: `PW_BLOB_DIR`, `PW_UPLOAD_MAX_BYTES`,
  `PW_UPLOAD_MAX_WIDTH`, `PW_UPLOAD_MAX_HEIGHT`.

## Amendment, 2026-10-25: an uploaded image is kept without its metadata (ADR-0383)

Corrections first. This ADR kept and served an upload's bytes as sent, so a
photo posted in the feed published its GPS position, camera, time, XMP,
IPTC and comments ("Not claimed": "stripping metadata (an Exif GPS position
is served as uploaded)"): a privacy defect, found by the dating-app design
note. Its "EXIF orientation outside JPEG: read as stored" was wrong in two
engines: Chromium and Firefox turn a PNG by its eXIf (WebKit does not), so a
turned PNG's box was sized unturned there. `sniff.rs` read the last of a
JPEG's two Exif structures, where every engine reads the first. And
`Shared::lease` lost the bytes of the same image attached twice by one
session (§4's lease, fixed). ADR-0383 (track `upload-metadata`) has the host
rewrite every upload before it is kept: each format's container walked, what
decoding, colour, animation and a JPEG's turn need kept byte for byte (the
turn as an Exif holding `Orientation` alone), everything else dropped, a
container that does not parse refused whole; what is kept is measured,
keyed, kept and served. §5's "Served safely" now serves those bytes; the two
"Not claimed" items above no longer hold.
