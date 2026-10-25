# ADR-XXXX: an uploaded image is kept and served without what names a place, a device, a person or a time

Status: proposed by track `upload-metadata` (`track/upload-metadata`), under
the integrator's ruling of 2026-10-25; amends ADR-0260 (and ADR-0260 says so
in its amendment of the same date). Date: 2026-10-25. Milestone: E14.

## Corrections first

1. **ADR-0260 published where a photo was taken.** It kept and served an
   upload's bytes as they were sent ("Not claimed: ... stripping metadata
   (an Exif GPS position is served as uploaded)"). A photo taken on a phone
   and posted in the feed published, to everyone shown the post and to any
   cache under `immutable`, its GPS position, the camera, when it was taken,
   and whatever its XMP, IPTC or a comment named. Pleris claims privacy by
   construction; this was a privacy defect, not a missing feature. Found by
   the dating-app design note (2026-10-25).
2. **A PNG's eXIf turned it in two engines, in a box its markup sized
   unturned.** ADR-0260 read a PNG's size from its IHDR alone ("EXIF
   orientation outside JPEG ...: read as stored"). Chromium and Firefox turn
   a PNG by its eXIf `Orientation`; WebKit, as Playwright 1.58.0 builds it,
   does not. A PNG stored 6 by 4 whose eXIf says 6 was shown 4 by 6 in the
   first two, so the page's reserved box was wrong there, and the engines
   disagreed. No engine turns a WebP by its EXIF. (The spec's EVIDENCE
   lines, below.)
3. **Of a JPEG's two Exif structures, the host read the last; every engine
   reads the first.** `sniff.rs` let each Exif APP1 before the frame header
   overwrite the turn. A JPEG stored 8 by 5 with an Exif saying 6, then a
   second saying 1, was measured 8 by 5 and shown 5 by 8 in all three
   engines.
4. **A session that attached the same image twice lost it.** `Shared::lease`
   put the new bytes in staging, ended the session's earlier lease, and only
   then counted the new lease's hold: where both named the same bytes,
   ending the earlier dropped the blob's last hold and deleted the bytes just
   put. The new lease's preview answered 404 and its post's `keep` failed
   ("lease has no bytes"). The test `the_same_image_attached_twice_is_kept`
   failed on master; the hold is now taken first (its own commit).

## Context

- **ADR-0260's path** (§2, §4, §5): the kind is sniffed and the header read
  from the bytes; the bytes are leased by their SHA-256, put in the
  deployment's `BlobStore` when a post commits, and served from it at their
  key, `immutable`.
- **The integrator's ruling (2026-10-25)**: an uploaded image is stored and
  served without metadata that can identify a place, a device, a person or
  a time. The host strips it at upload, generically, before the image is
  kept, with no new crate: the containers are simple at the byte level, so
  a careful rewriter, not a decoder. Per format: JPEG drops APP1 (Exif,
  XMP), APP13, APP12, other APPn and COM, keeps what decoding and colour
  need, and keeps a photo's turn as an Exif holding `Orientation` alone; PNG
  drops eXIf, tEXt, zTXt, iTXt and tIME and keeps what rendering needs; WebP
  drops EXIF and XMP and clears their flags; GIF drops comments and every
  application extension but Netscape's loop. A file that does not parse is
  refused, never kept half-rewritten; the rewrite is idempotent; what is
  kept is what is served.
- **A new ADR, not a section of ADR-0260**: this is a decision of its own
  (a host behaviour every upload passes through, its research, its
  alternatives, its findings and its evidence), as ADR-0261 was ADR-0260's
  next one; ADR-0260 is already long; and its amendment points here so
  that neither its "Not claimed" nor its "read as stored" is read as
  current.

## Research

Each read 2026-10-25.

- **JPEG.** ITU-T T.81, Table B.1 (markers; SOFn, DHT, DAC, DQT, DNL, DRI,
  DHP, EXP; APPn, COM; JPGn and RESn reserved), B.1.1.2 (fill bytes) and
  B.1.1.5 (a scan's entropy-coded data: `FF 00` stuffed, RSTm within it).
  JFIF 1.02 (E. Hamilton, w3.org's `jfif3.pdf`): APP0 is "identifier,
  version, units, Xdensity, Ydensity, Xthumbnail, Ythumbnail, (RGB)n", a
  JFXX APP0 holds a thumbnail in another coding, and "Application-specific
  APP0 marker segments ... do not affect the decodability or displayability".
  libjpeg-turbo's `jdmarker.c`: `APP0_DATA_LEN 14` and `APP14_DATA_LEN 12`
  ("Length of interesting data"); `examine_app14` reads "Adobe", version,
  flags0, flags1 and `transform = data[11]`, which picks the colour
  transform; DHP, EXP, JPGn and RESn are fatal there. ICC.1, Annex B: an
  APP2 profile is "ICC_PROFILE\0", a sequence number, a count. TIFF 6.0 §2
  for the Exif kept: header, IFD0 of 12-byte entries, a SHORT's value
  left-justified, no next IFD. CSS Images 3 (`image-orientation:
  from-image`, ADR-0260's research): a browser turns a JPEG by `Orientation`.
- **PNG**, W3C PNG third edition: the chunk list (critical IHDR, PLTE, IDAT,
  IEND; tRNS; colour space cHRM, gAMA, iCCP, sBIT, sRGB, cICP, mDCV, cLLI;
  text iTXt, tEXt, zTXt; bKGD, hIST, pHYs, sPLT, eXIf; tIME; animation
  acTL, fcTL, fdAT); an unknown critical chunk: the decoder "shall indicate
  to the user that the image contains information it cannot safely
  interpret"; an ancillary one it "can safely ignore".
- **WebP**, RFC 9649: VP8X's `Rsv|I|L|E|X|A|R` (bits from the most
  significant: ICC `20`, alpha `10`, Exif `08`, XMP `04`, animation `02`),
  "MUST be 0" for the reserved; a chunk of odd size padded by a zero byte;
  ANMF's 16 bytes, then "an optional alpha subchunk", "a bitstream
  subchunk" and "an optional list of unknown chunks"; FourCCs `EXIF` and
  `XMP ` ("an ASCII space"); unknown chunks "Readers SHOULD ignore".
- **GIF89a** (w3.org): a graphic control's "scope ... is the first graphic
  rendering block to follow", its block size 4; a comment "under no
  circumstances should ... disrupt or interfere with the processing"; an
  application extension's 11 bytes (identifier and authentication code)
  and its sub-blocks; the trailer `3B`. Netscape's loop is not GIF89a's:
  Firefox's `nsGIFDecoder2.cpp` reads "NETSCAPE2.0" or "ANIMEXTS1.0", its
  sub-block 1 the loop count, ignores sub-block 2, and skips "any other
  type of extension block, including comment and plain text blocks".
- **What the engines show of the files as sent** (this track's spec,
  Playwright 1.58.0's Chromium, Firefox and WebKit, 2026-10-25): a PNG
  stored 6 by 4 with eXIf `Orientation` 6, shown 4 by 6, 4 by 6, 6 by 4; a
  WebP stored 12 by 9 with EXIF 6, 12 by 9 in each; a JPEG stored 8 by 5
  with Exif 6 then 1, 5 by 8 in each.

## Decision

### 1. The host rewrites every upload before it keeps it

- **Where**: `Shared::upload` (`uploads.rs`), after ADR-0260's checks, whose
  refusals are unchanged and come first (session, origin, length, form,
  size, kind, header). `uploads/strip.rs` rewrites the bytes; what it keeps
  is measured again, since that is what a browser is served; leased by its
  own SHA-256; put in the deployment's storage by `keep`; and served. From
  the lease on there is one set of bytes.
- **Generic**: every `upload` a program declares, with nothing in the
  language changed and nothing a program can turn off.
- **A rewriter, not a decoder**: no pixel is decoded or re-encoded; what is
  kept is copied byte for byte, so an image's quality is unchanged.
- **An allow-list**: each format keeps what decoding, colour, animation and
  a photo's turn need, and drops every other part, whatever a writer
  called it (C2PA's APP11 and `caBX`, Apple's private chunks, a vendor's
  APPn), not only the metadata known today.
- **Refused whole** (422, "The file is not a whole JPEG image: it ends
  before its EOI."), never kept half-rewritten: a container that does not
  parse. Newly refused, where ADR-0260 kept them: a JPEG that ends before
  EOI, a PNG before IEND, a GIF before its trailer; a WebP chunk past its
  RIFF, an ANMF shorter than its header; a PNG chunk type not four letters,
  or a critical one this does not know; a GIF block that is no image,
  extension or trailer, or a graphic control not of four bytes.
- **Idempotent**: what is kept is rewritten to itself; every real encoder's
  fixture (ADR-0260's) is kept as it was.
- **The budget** (ADR-0260 §3a) spends the bytes sent, since what the server
  read and rewrote is what it cost; the staging's cap counts them too, over
  what is held and never under.

### 2. What each format keeps

| | kept | dropped | refused |
|---|---|---|---|
| JPEG | SOFn, DHT, DAC, DQT, DRI, DNL, DHP, EXP; each SOS and its scan byte for byte; JFIF's APP0, its twelve bytes and no thumbnail; APP2's ICC profile; Adobe's APP14, its twelve bytes; an Exif holding `Orientation` alone | every other APPn (Exif's APP1 with its GPS IFD, maker notes and thumbnail; XMP; MPF; FlashPix; IPTC's APP13; C2PA's APP11; APP12, APP15), COM, JFXX, reserved markers, TEM and RSTm outside a scan, everything after EOI | ends before EOI; a second SOI; a stuffed zero or no marker where a marker is; a length under 2 |
| PNG | IHDR, PLTE, IDAT, IEND, tRNS, cHRM, gAMA, iCCP, sRGB, cICP, mDCV, cLLI, acTL, fcTL, fdAT, each with its CRC | tEXt, zTXt, iTXt, tIME, eXIf, pHYs, sBIT, bKGD, hIST, sPLT, every unknown ancillary chunk, everything after IEND | ends before IEND; a type not four letters; an unknown critical chunk |
| WebP | simple: its image chunk; extended: VP8X (its ten bytes, Exif and XMP flags cleared), ICCP, ANIM, ANMF (its header, ALPH, `VP8 `, VP8L), ALPH, `VP8 `, VP8L; the RIFF's size rewritten, odd chunks padded | EXIF, `XMP `, unknown chunks (in a frame too), anything a simple file has after its image, everything past the RIFF's size | a chunk past its RIFF; an ANMF under 16 bytes |
| GIF | the header, the logical screen and its colour table; each image whole; a graphic control's four bytes; `NETSCAPE2.0`'s or `ANIMEXTS1.0`'s loop count | comments, plain text, every other application's extension (XMP's `XMP DataXMP`, an ICC profile's `ICCRGBG1012`), unknown labels, a loop's or a graphic control's other sub-blocks, everything after the trailer | ends before the trailer; an unknown block; a graphic control not of four bytes |

- **A photo's turn.** A browser turns a JPEG by its Exif `Orientation`, and
  dropping it would show a phone's portrait photo sideways. The first Exif
  APP1 whose `Orientation` reads, before the first scan, is the one a
  browser reads (correction 3); where it says 2 to 8 the rewrite writes an
  APP1 that holds that one tag: `Exif\0\0`, a big-endian TIFF header, IFD0
  of one entry (`0112`, SHORT, count 1), no next IFD, 32 bytes. Pillow's
  turned fixture is exactly that already. 1, or a value out of range, is
  not written: a browser shows either unturned.
- **A thumbnail is dropped wherever it is** (Exif's IFD1, JFIF's, JFXX): a
  thumbnail is the picture as it was before an edit cropped it.
- **ICC profiles are kept** (APP2, iCCP, ICCP), as the ruling says, since
  colour needs them; see Not claimed for what a profile's header says.
- **pHYs is dropped**: no engine needs it to decode or colour an image, and
  a screenshot's pixels per metre names the screen that made it (a Retina
  display's 144 ppi). sBIT, bKGD, hIST and sPLT are dropped too: no
  browser renders by them, and sPLT carries a name.
- **An unknown critical PNG chunk is refused**, not dropped: dropping it
  would change what the image is (the PNG specification's "cannot safely
  interpret").
- **GIF's plain text is dropped** and a graphic control before it kept: it
  is text a writer chose, Firefox's decoder skips it as it skips a comment,
  and keeps a graphic control's values for the image that follows (they
  are cleared only in `EndImageFrame`), which is what the kept file says.
  Whether Chromium or WebKit draws plain text was not checked.
- **No CRC is checked**: a kept chunk is copied with its CRC, and a decoder
  judges it, as before.

### 3. Images kept before this

- **The development blob store holds no production data**, and this track
  cleans nothing.
- **A deployment with images kept before this would be cleaned by a pass
  the host offers once** (described, not built): with uploads closed, so no
  claim is in flight, for each distinct `posts.image_key` (ADR-0261 serves
  only a blob a post names, so these are every blob served), read the blob
  and rewrite it; where the bytes change, put the rewritten bytes (a new
  key), set every post naming the old key to the new one and its width and
  height to the rewritten bytes' in one transaction, then delete the old
  blob (`collect`) and have the deployment's CDN purge
  `<serves>/<old key>.<kind>`, which `immutable` let anyone keep. A blob
  that does not parse is reported and left. A second pass changes nothing.
  A copy a browser kept is out of reach, as ADR-0261 says of a deleted
  post's. Not built: no deployment holds images, and it is a migration
  across a database and a blob store with its PostgreSQL tests, not small.

### 4. What `KNOWN_LIMITATIONS.md` and the capability matrix should say

The integrator's files; proposed wording:

- **KNOWN_LIMITATIONS**: remove "an Exif GPS position is served as
  uploaded" and "EXIF orientation outside JPEG: read as stored". Add: an
  uploaded image keeps its ICC profile whole, whose header names the
  profile's maker and device class and may name a model and a creation
  date (for the profiles phones embed, sRGB or Display P3, the profile's,
  the same in every photo of that maker); nothing in the pixels is touched
  (a face, a street sign, a screen in a reflection), nor data hidden in a
  JPEG's scan or past a GIF image's LZW end; an Ultra HDR or ISO 21496-1
  gain map goes with MPF and what follows EOI, so such a photo shows as its
  standard-range image; a GIF's ICC profile is dropped, its colours then
  taken as sRGB; images kept before this in a deployment are not
  rewritten (§3); a PNG or WebP is shown as stored, never turned by an
  Exif.
- **Capability matrix**, uploads: "metadata (Exif and its GPS position,
  XMP, IPTC, comments, text chunks, timestamps, thumbnails, trailing data)
  stripped by the host before an image is kept, for every upload; a JPEG's
  turn kept; colour profiles and animation kept; tested in Chromium,
  Firefox and WebKit."

## Found

- Corrections 2, 3 and 4 above, each by a test of this track.
- **master's `Cargo.lock` is not in cargo's order**: `pw-tasks` comes before
  `pw-table-memory` (added by `7b7edea6`, 2026-10-23). `run.sh`, `feed.sh`
  and `just ci`'s `cargo test --workspace` run cargo without `--locked`,
  which rewrites the file in its order, so a fresh worktree is dirty after
  its first browser build (here, after `run.sh` and `feed.sh`), and a
  recipe's `commit:` line, which asks `git diff --quiet HEAD`, would add
  "+ uncommitted changes" where that happens before it is recorded, which
  `ci_plan.py` (`RECORDED_AT`) reads as no commit at all: such evidence is
  never current. No evidence has been recorded on master since. Put in
  cargo's order on this branch, in its own commit (`c71acb8b`), exactly as
  cargo rewrote it, so `--locked` builds are unchanged.
- **No flake** in the runs here.

## Alternatives

- **Re-encoding each image** (decode, then encode the pixels): strips every
  container's metadata, and needs decoders and encoders (new crates), loses
  quality and changes sizes. Refused by the ruling.
- **A metadata crate**: a dependency for a few hundred lines of byte
  rewriting. Refused by the ruling.
- **A block-list** (drop Exif, XMP, IPTC, text chunks): keeps whatever a
  writer invents next. Each format's allow-list keeps what decoding needs.
- **Keeping Exif with its GPS, time and camera tags taken out**: keeps the
  maker notes (serial numbers among them), unknown tags and offsets. The
  Exif kept holds one tag.
- **Turning the pixels instead of keeping `Orientation`**: needs a JPEG
  codec (a lossless turn moves DCT blocks); dropping the tag would turn
  portraits sideways.
- **Refusing images with metadata**: refuses every phone's photo.
- **Stripping in the program**: every program would have to; the host does
  it once.

## Consequences

- **The bytes kept differ from those sent**: their key is theirs, so two
  exports of one photo with different metadata are one blob.
- **Some files ADR-0260 kept are refused** (Decision §1), each with why.
- **One pass and one buffer** the size of the upload, at most the
  declaration's bytes; nothing in the language, the runtime or a program
  changes.
- **The test helper `png(w, h)`** ends with IEND, since the host walks to
  it; the budget test varies a byte past IEND.

## Acceptance

Local, at `f3d83897` (before this ADR): the server's `uploads` and `blob`
tests, 68 passed, 18 of them this track's: the rewrite's 13 (each format
laden and kept exactly, malformed refused, every real encoder's fixture
kept as it was, and each laden file cut at every length and with each byte
changed, refused or kept and kept again unchanged), four at the upload
seam (leased, measured and kept as rewritten; refused whole; the same
image attached twice) and one over HTTP; `e2e/upload-metadata.spec.mjs`,
9 of 9 in Chromium, Firefox and WebKit; `just evidence-gates` (1066 tests);
a smoke of the mutation script (both baselines, two mutants killed); `just
ci`, green.

On GitHub, at `b64610b9`: verify run 40183150640 (dispatched with the 103
recipes `ci_plan.py --changed origin/master HEAD --current` planned, and
its three full browser jobs) succeeded, every recipe exiting 0; among them
`e14-upload-metadata` (718 s: baselines of 68 server tests and 9 browser
tests, then every mutant killed, since the script exits non-zero on a
survivor or a lost anchor), `e14-uploads` (766 s, its 34 controls run whole,
since its tests and its anchors' files changed) and
`e14-browser-host-map`. CI run 40183140413 at the same commit: green. The
run's evidence files were not fetched here (see the Report).

- `scripts/upload_metadata_mutations.py`, 55 mutants: each keeping one kind
  of metadata, breaking one thing the rewrite keeps, letting a file that
  does not parse through, or undoing a piece of the upload path; two
  killed by the browser suite alone.

## Not claimed

- See §4's KNOWN_LIMITATIONS: ICC profiles, pixels, hidden data, gain maps,
  a GIF's ICC profile, images kept before this.
- A JPEG's scans, restart intervals or Huffman codes validated, or a PNG's
  chunk order or CRCs: a decoder judges what is kept, as before.
- The multipart part's file name (never read, ADR-0260) and the request's
  headers (never kept).
- ADR-0260's own: decompression bombs past the declared size, animated
  frames checked beyond GIF's screen and WebP's canvas, AVIF.

## Questions for the integrator

1. **The cleaning pass** (§3): queue it for the first deployment that keeps
   images, or never, since none does?
2. **pHYs** dropped by this track's judgment (§2): agreed?
3. **An opt-out** (a program that shows a photo's camera, say): not
   proposed; the host strips for every upload. Agreed?
4. **`Cargo.lock`'s order** (Found): sorted here in its own commit; and
   should `run.sh`, `feed.sh` and `just ci` pass `--locked`, so a lockfile
   out of order fails loudly rather than dirtying a checkout?

## Report

**Built.** `uploads/strip.rs` (the rewrite of JPEG, PNG, WebP and GIF, and
its tests), its wiring in `Shared::upload`, the lease fix, tests at the
upload seam and over HTTP, `e2e/upload-metadata.spec.mjs` and its host
registration, `scripts/upload_metadata_mutations.py`,
`just/upload-metadata.just` and its import, `Cargo.lock` in cargo's order,
this ADR and ADR-0260's amendment.

**Evidence.** Verify run 40183150640 at `b64610b9`, all 103 planned recipes
green with the three full browser jobs; CI 40183140413 green; locally the
server's 68 tests, the spec's 9 of 9 in three engines and `just ci`. The
run's files are not in `docs/evidence/` yet: `just evidence-fetch` reads
artifacts through `gh`, which is logged out in this PC's WSL, and the
brief said to download nothing, so the fetch is the integrator's (or a
word to this track).

**Dependencies.** None new.
