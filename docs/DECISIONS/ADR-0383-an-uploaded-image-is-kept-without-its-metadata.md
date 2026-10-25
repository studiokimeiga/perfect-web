# ADR-0383: an uploaded image is kept and served without what names a place, a device, a person or a time

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
5. **This ADR's first version kept an ICC profile whole, and a JPEG's
   density; two of its parser's regressions passed every test; its WebP
   walk listed every chunk; and it said a browser reads the first readable
   Exif** (the independent review of 2026-10-25, below): a screenshot's
   profile named its display's model and serial number and when it was
   made, and a JPEG kept its screen's 144 dpi where a PNG's pHYs was
   dropped. Each is fixed below, with its test and its mutation control.

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
| JPEG | SOFn, DHT, DAC, DQT, DRI, DNL, DHP, EXP; each SOS and its scan byte for byte, its restart markers with it; JFIF's APP0, its version, no units, a density of 1 by 1 and no thumbnail; the ICC profile of the APP2 chunks before the first scan, rewritten (§2a) and split again at 65,519 bytes; Adobe's APP14, its twelve bytes; an Exif holding `Orientation` alone | every other APPn (Exif's APP1 with its GPS IFD, maker notes and thumbnail; XMP; MPF; FlashPix; IPTC's APP13; C2PA's APP11; APP12, APP15), COM, JFXX, JFIF's units and densities, ICC chunks that libjpeg would not read as one profile or after the first scan, reserved markers, TEM and RSTm outside a scan, everything after EOI | ends before EOI; a second SOI; a stuffed zero or no marker where a marker is; a length under 2 |
| PNG | IHDR, PLTE, IDAT, IEND, tRNS, cHRM, gAMA, sRGB, cICP, mDCV, cLLI, acTL, fcTL, fdAT, each with its CRC; the first iCCP, named "ICC profile", its profile rewritten (§2a), its CRC computed | tEXt, zTXt, iTXt, tIME, eXIf, pHYs, sBIT, bKGD, hIST, sPLT, a second iCCP, every unknown ancillary chunk, everything after IEND | ends before IEND; a type not four letters; an unknown critical chunk |
| WebP | simple: its image chunk; extended: VP8X (its ten bytes, Exif and XMP flags cleared, its ICC flag where a profile is kept), the first ICCP rewritten (§2a), ANIM, ANMF (its header, ALPH, `VP8 `, VP8L), ALPH, `VP8 `, VP8L; the RIFF's size rewritten, odd chunks padded | EXIF, `XMP `, a second ICCP, unknown chunks (in a frame too), anything a simple file has after its image, everything past the RIFF's size | a chunk past its RIFF; an ANMF under 16 bytes |
| GIF | the header, the logical screen and its colour table; each image whole; a graphic control's four bytes; `NETSCAPE2.0`'s or `ANIMEXTS1.0`'s loop count | comments, plain text, every other application's extension (XMP's `XMP DataXMP`, an ICC profile's `ICCRGBG1012`), unknown labels, a loop's or a graphic control's other sub-blocks, everything after the trailer | ends before the trailer; an unknown block; a graphic control not of four bytes |

- **A photo's turn.** A browser turns a JPEG by its Exif `Orientation`, and
  dropping it would show a phone's portrait photo sideways. The rewrite
  reads the first Exif APP1 whose `Orientation` reads, before the first
  scan, and where it says 2 to 8 writes an APP1 that holds that one tag:
  `Exif\0\0`, a big-endian TIFF header, IFD0 of one entry (`0112`, SHORT,
  count 1), no next IFD, 32 bytes. Pillow's turned fixture is exactly that
  already. 1, or a value out of range, is not written: a browser shows
  either unturned. What the engines read of a file as sent differs
  (measured in Playwright 1.58, the spec's EVIDENCE lines; amended by the
  independent review, below): Firefox reads only a JPEG's first APP1, of
  any kind, so an Exif after an XMP APP1 does not turn it there; Chromium
  and WebKit read the first Exif APP1. Of two Exif APP1s, all three read
  the first (correction 3). The file kept holds one APP1, the turn alone,
  so all three engines show it alike (the spec's `xmp-first.jpg`).
- **A thumbnail is dropped wherever it is** (Exif's IFD1, JFIF's, JFXX): a
  thumbnail is the picture as it was before an edit cropped it.
- **ICC profiles are kept for their colour, and rewritten** (APP2, iCCP,
  ICCP; §2a, from the independent review): colour needs them, and a
  display's names its model, its serial number and a time.
- **pHYs is dropped, and JFIF's density written 1 by 1**: no engine needs
  either to decode or colour an image, and a screenshot's pixels per metre
  or dots per inch name the screen that made it (a Retina display's 144
  ppi). JFIF's APP0 keeps its version and says no units and a density of 1
  by 1, the pixels square (the independent review's finding 2). sBIT, bKGD,
  hIST and sPLT are dropped too: no browser renders by them, and sPLT
  carries a name.
- **An unknown critical PNG chunk is refused**, not dropped: dropping it
  would change what the image is (the PNG specification's "cannot safely
  interpret").
- **GIF's plain text is dropped** and a graphic control before it kept: it
  is text a writer chose, Firefox's decoder skips it as it skips a comment,
  and keeps a graphic control's values for the image that follows (they
  are cleared only in `EndImageFrame`), which is what the kept file says.
  Whether Chromium or WebKit draws plain text was not checked.
- **No CRC is checked**: a kept chunk is copied with its CRC, and a decoder
  judges it, as before. An iCCP rewritten has its CRC computed.

### 2a. An ICC profile, kept for its colour (the independent review)

`uploads/icc.rs` reads a profile as ICC.1 lays it out, in version 2
(ICC.1:2001-04) as in version 4 (ICC.1:2022), and in the versions before 2,
laid out alike: a 128-byte header; a tag table (a count, then twelve bytes
a tag: signature, offset from the profile's start, size); each tag's data,
its type's signature and four reserved bytes first. What is kept is
rebuilt from it:

- **the header**, its size rewritten, with zeros where it says when the
  profile was created (bytes 24 to 35), the device's maker, model and
  attributes (48 to 63), the profile's ID (84 to 99, an MD5 of the profile
  as it was) and the reserved bytes (100 to 127); its class, colour space,
  connection space, version, flags, rendering intent, illuminant and
  creator as they were;
- **each tag of a colour or rendering type**, byte for byte: `XYZ `, `curv`,
  `para`, `sf32`, `mft1`, `mft2`, `mAB `, `mBA `, `mpet`, `chrm`, `clro`,
  `meas`, `sig `, `view`, `cicp`;
- **each tag of a text type** (`desc`, `mluc`, `text`: the profile's
  description, the device's maker's and model's, the copyright, the viewing
  conditions', a target's) as its own type holding one fixed text, "ICC
  profile": a version 2 `desc` of 102 bytes (ASCII, no Unicode or
  ScriptCode text), an `mluc` of one English record, or a `text`;
- **no tag of any other type**: Apple's `mmod` (a display's maker, model and
  serial number), `dtim` (`calt`, when it was calibrated), `dict`, `pseq`,
  `psid`, `data`, `ncl2`, `clrt`, `vcgt`, and a writer's own. Colour is
  computed from none of them; this goes past the ruling, which named text
  tags, by the track's allow-list (§1): a serial number is not text.

Tags that shared their data share it still, the data laid out in its order
and four-byte aligned, so what is kept is rebuilt to itself. **A profile that
does not parse is dropped**, and its image is then shown as sRGB: shorter
than its header says, its size under 132, no `acsp`, a major version over 4
(iccMAX), more than 1,024 tags, a tag in the table or outside the profile or
shorter than its type's signature, two tags' data overlapping but not the
same bytes, over 4 MiB, or made over 4 MiB by its fixed texts. Bytes after
the size a profile's header says are not kept with it.

- **JPEG**: its APP2 chunks before the first scan are read as libjpeg-turbo's
  `jpeg_read_icc_profile` reads them (each numbered 1 to the count each
  gives, the same count in all, no number twice, none missing; an APP2 under
  14 bytes is no chunk), joined, rewritten, and written back after JFIF's
  APP0 and the turn, in as few chunks of at most 65,519 bytes as hold it.
  Chunks that do not make one profile are dropped, as libjpeg drops them.
- **PNG**: the first iCCP's name (free text, a person's name, say) becomes
  "ICC profile"; its profile is inflated with `miniz_oxide` (0.9.1, already
  `pw-dev-server`'s dependency for kiokun's words: no new crate) to at most
  4 MiB, rewritten, deflated again (level 6) and its CRC computed. An iCCP
  with no name or a name past 79 bytes, another compression method, a
  stream that is cut or inflates past 4 MiB, or a profile that does not
  parse is dropped; a second iCCP is dropped (a PNG has one).
- **WebP**: the first ICCP is rewritten; a second dropped; VP8X's ICC flag
  cleared where none is kept.
- **4 MiB** is this track's bound on what a profile may cost to read (a PNG's
  is inflated): phones' and displays' profiles are of a few hundred bytes to
  a few kilobytes, a printer's with large tables a few megabytes.

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
  uploaded image keeps its ICC profile's colour, its colour tags byte for
  byte (a calibrated display's measured curves and primaries can tell its
  panel from another's), its header's class, colour spaces, version,
  flags, intent and creator; its descriptions, device fields, creation
  time, ID and other tags are not kept (§2a), and a profile that does not
  parse is dropped, its image then shown as sRGB; nothing in the pixels is touched
  (a face, a street sign, a screen in a reflection), nor data hidden in a
  JPEG's scan or past a GIF image's LZW end; an Ultra HDR or ISO 21496-1
  gain map goes with MPF and what follows EOI, so such a photo shows as its
  standard-range image; a GIF's ICC profile is dropped, its colours then
  taken as sRGB; images kept before this in a deployment are not
  rewritten (§3); a PNG or WebP is shown as stored, never turned by an
  Exif.
- **Capability matrix**, uploads: "metadata (Exif and its GPS position,
  XMP, IPTC, comments, text chunks, timestamps, thumbnails, densities,
  trailing data) stripped by the host before an image is kept, for every
  upload; a JPEG's turn kept; colour profiles kept for their colour without
  their descriptions, device fields or times; animation kept; tested in
  Chromium, Firefox and WebKit."

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
- **WebKit keeps a page's CSP in the `about:blank` it goes to** (Playwright
  1.58, found by the review's colour check): after the feed's page,
  `page.goto("about:blank")` refuses a `data:` image, a
  `securitypolicyviolation` naming the feed's `img-src 'self'`; Chromium and
  Firefox load it, and so does WebKit on a new page's `about:blank`. The
  spec decodes on a new page.
- **Which engines render by an embedded profile** (the spec's EVIDENCE
  lines, Playwright 1.58): Chromium by the JPEG's, the PNG's and the WebP's;
  WebKit by the JPEG's and the PNG's, not the WebP's ICCP; Firefox, as
  Playwright builds and runs it, by none. Where an engine renders by it,
  the rewritten profile gives the same colour as the profile sent.

## Independent review (2026-10-25)

An independent review found five things, each reproduced by its reviewer;
the integrator ruled on each (2026-10-25). A second worker fixed them on
this branch, each test written first and seen failing on the code before
(the restart and palette fixtures pass on it: their mutants are what
fails). Each fix has its mutation control in
`scripts/upload_metadata_mutations.py`.

1. **ICC profiles can identify** (medium). A JPEG's APP2 profile, a PNG's
   iCCP (whose name is free text) and a WebP's ICCP were kept whole: a
   screenshot's display profile names its model (`desc`, `dmnd`, `dmdd`),
   may hold its serial number (Apple's `mmod`), and its header holds a
   creation time and an ID. **Ruling**: scrub, don't drop. **Fix**:
   `uploads/icc.rs` and §2a: the header's time, device fields and ID
   zeroed; each text tag a fixed text of its type; each colour tag byte for
   byte; every other tag dropped (beyond the ruling: Apple's serial number
   is not text); a profile that does not parse dropped, its image then
   shown as sRGB. The PNG's iCCP is renamed "ICC profile", inflated with
   `miniz_oxide`, which `pw-dev-server` already takes (so no profile is
   dropped for want of an inflater, and no crate is added), rewritten,
   deflated and given its CRC. **Tests**: `icc.rs`'s three (a display's
   profile in version 2 and 4 with a person's name, a camera, a serial
   number, a copyright, `mmod`, `calt`, a creation time and an ID planted:
   none survives, the header's colour fields and every colour tag byte for
   byte, the three curves still one datum, rebuilt to itself; the fixed
   texts byte for byte; ten malformed profiles dropped, 1,024 tags kept
   and 1,025 not, a profile a text would push past 4 MiB not kept); in
   `strip.rs`, each laden file's profile planted and none of it served, and
   `a_jpegs_icc_profile_is_read_as_libjpeg_reads_its_chunks`,
   `a_pngs_icc_profile_is_read_from_its_first_iccp`,
   `a_webps_icc_profile_is_read_from_its_first_iccp`; in the browser,
   `profiled.jpg`, `profiled.png` and `profiled.webp` served without any
   planted value (the PNG's profile inflated to be searched), shown at their
   size, and decoded to the colour of the files as sent in all three
   engines.
2. **A JPEG's density** (low). JFIF's units and densities were kept (a 144
   dpi survived) where PNG's pHYs is dropped. **Fix**: units 0, density 1
   by 1. **Test**: `a_jpegs_density_is_not_kept` (144, 57 per cm, 72 by
   300, an aspect of 2 to 1: each kept as 0, 1, 1); the laden JPEG's JFIF
   is at 144 dpi.
3. **Two parser regressions passed every test** (medium): a scan ended at
   a restart marker, which refuses every JPEG with restart intervals
   (common from phones' and cameras' hardware encoders); a GIF image's
   local colour table not skipped, which refuses every GIF with per-frame
   palettes. **Fix**: fixtures built from bytes in the tests,
   `restarted_jpeg` (16 by 8, a DRI of one MCU, an RST0 in its scan, its
   halves grey 64 and 192) and `local_table_gif` (its image's own palette,
   red and blue, over a black and white global one), each kept byte for
   byte (with a camera's metadata or a comment around them, dropped), and
   served whole and shown with their colours in three engines
   (`restarted.jpg`, `palettes.gif`); both mutants in the script.
4. **A WebP cost about seven times its bytes** (low, denial of service):
   `chunks()` listed every chunk (32 bytes for an empty chunk's 8). **Fix**:
   `Chunks`, an iterator; the simple file's remaining chunks are walked
   (and checked) without a list. **Test**:
   `a_webp_of_many_empty_chunks_is_walked_without_a_list`: a child process
   of the test binary strips a 5 MB file of 624,990 empty chunks, its peak
   resident set (`VmHWM`) read before and after, under a quarter of the
   input over it (0 kB here; the list put back adds 19 MB) and under two
   seconds (some 45 ms here). The codebase has no allocation counter, so
   the peak resident set is measured, as the ruling allowed; the walk is
   first run on a small file so that the pages of code it faults in (a
   megabyte or two) are not counted, which they were at first.
5. **A wrong claim** (low): §2 and the comment in `strip.rs` said the first
   Exif APP1 whose `Orientation` reads is the one a browser reads. In
   Playwright 1.58, Firefox reads only the first APP1 of any kind;
   Chromium and WebKit the first Exif. **Fix**: both reworded to what was
   measured; the spec's EVIDENCE lines now measure it (a JPEG whose first
   APP1 is XMP, then an Exif saying 6: shown 8 by 5 in Firefox and 5 by 8
   in Chromium and WebKit), and its `xmp-first.jpg` is served turned in all
   three.
6. **Evidence of record** is the integrator's at landing.

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
- **Dropping every ICC profile** (the review's alternative): no profile can
  name anything, but a Display P3 photo or a printer's CMYK image loses its
  colours. Refused by the ruling.
- **Rewriting a profile's texts in place**: keeps every offset, but a fixed
  text does not fit a tag shorter than it (a `text` of "sRGB"), so the text
  would vary with the tag. The profile is rebuilt instead, its colour tags'
  bytes copied, so one fixed text fits every tag.
- **Keeping a profile's tags of other types** (as the ruling names only
  text tags): keeps Apple's `mmod`, which holds a display's serial number,
  and a calibration's time. An allow-list of colour types drops them.

## Consequences

- **The bytes kept differ from those sent**: their key is theirs, so two
  exports of one photo with different metadata are one blob.
- **An image with an ICC profile is kept with its profile rebuilt**, so its
  bytes differ from those sent even when it holds nothing else; a PNG's
  iCCP is deflated again, its size changed. A profile that does not parse
  is dropped, and its image is shown as sRGB.
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

**The independent review's fixes** (local, 2026-10-25, the PC idle): the
server's `uploads` and `blob` tests, 78 passed (ten of them the review's:
seven in `strip.rs`, three in `icc.rs`); `e2e/upload-metadata.spec.mjs`, 12
of 12 in Chromium, Firefox and WebKit (four tests, one of them new, and
the first with six more files); `cargo clippy -p pw-dev-server
--all-targets -D warnings`, `cargo fmt --check`,
`scripts/mutation_anchors.py`, `scripts/case-check.sh` and `just
evidence-gates`, each passing; and `scripts/upload_metadata_mutations.py`
run whole, as `just e14-upload-metadata` runs it (`CARGO_INCREMENTAL=0`):
both baselines green (78 server tests, 12 browser tests), then **91 of 91
mutants killed**: 36 added for the review (each finding's, and each check
`icc.rs` makes) and 8 re-anchored on code the review moved, two killed by
the browser suite alone. `just ci` is run before the push, which is made
only on green. Evidence of record is the integrator's at landing.

## Not claimed

- See §4's KNOWN_LIMITATIONS: an ICC profile's colour tags, pixels, hidden
  data, gain maps, a GIF's ICC profile, images kept before this.
- A JPEG's scans, restart intervals or Huffman codes validated, or a PNG's
  chunk order or CRCs: a decoder judges what is kept, as before. Likewise a
  kept colour tag's contents: a colour management module judges them.
- That every engine renders by a profile: Firefox, as Playwright 1.58
  builds and runs it, renders by none, so the colour check is vacuous
  there, and WebKit by no WebP ICCP (Found).
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
5. **An ICC profile, past the ruling** (§2a), each this worker's choice,
   each cheap to reverse: tags of a type neither colour nor text are
   dropped (Apple's `mmod` holds a serial number, `calt` a time); the
   header's device maker, model and attributes are zeroed besides its
   creation time and ID. Agreed?
6. **A profile's bounds** (§2a): at most 4 MiB and 1,024 tags, and no two
   tags' data overlapping unless they are the same bytes; a profile past
   them is dropped and its image shown as sRGB. None of the profiles a
   phone or a display embeds comes near them; agreed?
7. **What browsers do not read is dropped**: ICC chunks after a JPEG's
   first scan, a PNG's second iCCP, a WebP's second ICCP. Agreed?

## Report

**Built.** `uploads/strip.rs` (the rewrite of JPEG, PNG, WebP and GIF, and
its tests), its wiring in `Shared::upload`, the lease fix, tests at the
upload seam and over HTTP, `e2e/upload-metadata.spec.mjs` and its host
registration, `scripts/upload_metadata_mutations.py`,
`just/upload-metadata.just` and its import, `Cargo.lock` in cargo's order,
this ADR and ADR-0260's amendment. After the independent review:
`uploads/icc.rs` (an ICC profile kept for its colour alone) and its use in
the three formats, JFIF's density, the lazy WebP walk, the restart and
palette fixtures, the spec's colour check and six files, 36 more mutation
controls, and this ADR's §2a and review section.

**Evidence.** Verify run 40183150640 at `b64610b9`, all 103 planned recipes
green with the three full browser jobs; CI 40183140413 green; locally the
server's 68 tests, the spec's 9 of 9 in three engines and `just ci`. The
run's files are not in `docs/evidence/` yet: `just evidence-fetch` reads
artifacts through `gh`, which is logged out in this PC's WSL, and the
brief said to download nothing, so the fetch is the integrator's (or a
word to this track). After the review, locally: the server's 78 tests, the
spec's 12 of 12 in three engines, 91 of 91 mutants killed, and `just ci`
before the push.

**Dependencies.** None new: the PNG's iCCP is inflated and deflated with
`miniz_oxide` 0.9.1, which `pw-dev-server` already takes (its version
checked in `Cargo.lock` and its API, `decompress_to_vec_zlib_with_limit`
and `compress_to_vec_zlib`, in the crate's source in cargo's registry).

## Numbered (2026-10-25)

- **Numbered ADR-0383** by the integrator's delegate, which lands this
  track by pull request (the integrator's brief of 2026-10-25): the next
  number after master's ADR-0382, which no branch on origin, open pull
  request or worktree had taken. Numbered from `58591617`
  (`track/upload-metadata`) after the evidence of its verify 40203355758
  was fetched (`818f4278`). `ADR-XXXX` reads `ADR-0383` in the track's
  code, tests, spec, mutation script, recipe and evidence (14 mentions in
  8 files) and in ADR-0260's amendment (2), as in this file's name and
  title.
- **The integrator's answers** (2026-10-25) to the questions above: 1, the
  cleaning pass is queued for the production track (NEXT); 2, pHYs
  dropped: agreed; 4, yes: a follow-up passes `--locked` in `run.sh`,
  `feed.sh` and `just ci`, with a gate that every cargo call in a recipe
  or a script is locked (NEXT). Questions 3 and 5 to 7 remain open.
- **Evidence of record**: verify 40203355758 at `58591617` (the 103
  recipes its push planned, `e14-upload-metadata` with 91 of 91 mutants
  killed among them, and the three full browser jobs), fetched in
  `818f4278`; CI 40203355686 at the same commit. Numbering rewrote lines
  that `e14-upload-metadata` and other recipes read, so the pull
  request's own verify, at its head, runs again each recipe the numbering
  reaches (ADR-0281).
- **master moved after the numbering**: PR #53 (`4345726a`, D2's read
  path) merged at 18:37Z and was merged here (`c488d981`) without a
  conflict. It also put master's `Cargo.lock` in cargo's order, the order
  `c71acb8b` gives it, so the lockfile of this ADR's Found is in order on
  master since then; the `--locked` follow-up stands (NEXT).
- **Fallow's audit** (the pull request's check, gate new-only) refused
  three new functions of `e2e/upload-metadata.spec.mjs`, each of
  cyclomatic complexity 5, so a CRAP score of 30 at its threshold of 30;
  each was split, its behaviour unchanged (`fffe9e38`), and the spec
  passes 12 of 12 in the three engines as before. `just preflight` does
  not run Fallow (NEXT).
