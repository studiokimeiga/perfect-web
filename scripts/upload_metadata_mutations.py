#!/usr/bin/env python3
"""Mutation controls for track `upload-metadata` (ADR-0383, amending ADR-0260):
an uploaded image is kept and served without what names a place, a device, a
person or a time.

Each mutant keeps one kind of metadata, breaks one thing the rewrite keeps,
or undoes one piece of how the upload path uses it, and the tests must then
fail:

- JPEG: Exif's whole APP1 (its GPS position, camera and time), XMP, IPTC's
  APP13 or a comment kept; JFIF's thumbnail kept; what follows EOI kept; the
  turn not written back (killed in the browser); an Exif after the first
  scan read, or the last Exif's turn kept where a browser reads the first; a
  scan's data, the quantization tables, the ICC profile or Adobe's APP14 not
  kept; Adobe's APP14 kept past its twelve bytes; a JPEG that ends before
  its EOI kept;
- PNG: tEXt, zTXt, iTXt, tIME, eXIf, pHYs or a private chunk kept; what
  follows IEND kept; PLTE, tRNS, iCCP or fdAT not kept; a critical chunk it
  does not know dropped where it is refused; a PNG without IEND kept;
- WebP: EXIF, `XMP ` or an unknown chunk kept; VP8X's Exif or XMP flag left
  set; an animated frame's unknown chunk kept; ICCP or ANIM not kept; the
  RIFF's size not rewritten; an odd chunk not padded; a chunk past its RIFF
  kept;
- GIF: a comment, plain text or another application's extension (XMP's)
  kept; a loop extension's other sub-blocks, or a graphic control's, kept;
  what follows the trailer kept; the loop count or the graphic control not
  kept; `ANIMEXTS1.0` not read as a loop; a GIF without its trailer kept;
- the upload path: the bytes as sent leased where the rewritten are (killed
  in the browser); a file the rewrite refuses kept as sent; an image
  measured as sent, not as kept; a user's hourly bytes counting what is kept
  where the bytes sent are; a lease replaced by the same bytes forgetting
  them (the track's finding in `Shared::lease`);
- the independent review of 2026-10-25: an ICC profile (`uploads/icc.rs`)
  keeping its creation time, its device's maker and model, its ID, a
  `desc`, `mluc` or `text` tag's text, or a tag of another type (Apple's
  `mmod`, a calibration's `dtim`); a colour tag dropped or cut; a profile
  read that does not parse (shorter than it says, no `acsp`, a version over
  4, a tag in its table or shorter than its type, two tags' data
  overlapping, more than 1,024 tags) or kept larger than 4 MiB; a JPEG's
  ICC chunks kept as sent, or not kept, read after the first scan, read
  though their counts differ or a number repeats, or split one byte past an
  APP2; a PNG's iCCP not kept, kept as sent, keeping its name, a second kept,
  inflated without a bound, read with another compression method, with no
  name or a name past 79 bytes; a WebP's ICCP not kept, kept as sent, a
  second kept, VP8X's ICC flag left set where none is kept; JFIF's units and
  densities kept (finding 2); a scan ended at a restart marker, a GIF
  image's local colour table not skipped (finding 3: each refuses every
  such file); a WebP's chunks listed before they are walked (finding 4:
  four times its bytes in memory).

The tests: the server's (`uploads` and `blob`: `uploads/strip.rs`,
`uploads.rs`, `tests/uploads.rs`, `blob.rs`), and for the mutants marked
`browser`, the browser suite `e2e/upload-metadata.spec.mjs` alone in three
engines, on a server built from the mutant. Run from the repository root;
`just e14-upload-metadata` records the output. The source is restored after
every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
STRIP = ROOT / "spikes/own-renderer/server/src/uploads/strip.rs"
ICC = ROOT / "spikes/own-renderer/server/src/uploads/icc.rs"
UPLOADS = ROOT / "spikes/own-renderer/server/src/uploads.rs"

RUST = "rust"
BROWSER = "browser"

# The catch-alls a mutant adds an arm before.
JPEG_REST = "            // Every other APPn, COM, JPGn and RESn.\n            _ => {}\n"
PNG_KEEPS = "        if PNG_KEPT.iter().any(|k| &k[..] == kind) {\n"
WEBP_KEEPS = '                    b"ANIM" | b"ALPH" | b"VP8 " | b"VP8L" => {\n'
JFIF_KEPT = "                    jfif = Some([&data[..7], &[0, 0, 1, 0, 1, 0, 0]].concat());\n"
ICC_READ = "                    profile.add(data);\n"
ICCP_KEPT = "                if let Some(data) = iccp(&chunk[8..8 + length]) {\n"
ICCP_SCRUBBED = "                        if let Some(profile) = icc::scrub(data) {\n"
ZEROED = "const ZEROED: [(usize, usize); 3] = [(24, 36), (48, 64), (84, 128)];\n"
NEUTRAL = '        b"desc" | b"mluc" | b"text" => Keeps::Neutral,\n'
TAG_READ = "        if at < table || len < 4 || at.checked_add(len)? > size {\n"
SIGNED = '    if &p[36..40] != b"acsp" || p[8] > 4 {\n'
GIF_REST = (
    "                    // Comments (FE), plain text (01) and labels it does not know.\n"
    "                    _ => {}\n"
)
LOOP_COUNT = "                            out.extend([3, 1, count[0], count[1], 0]);\n"


def png_keeps(kind):
    return PNG_KEEPS.replace(" {\n", f' || kind == b"{kind}" {{\n')


def png_drops(kind):
    return PNG_KEEPS.replace(" {\n", f' && kind != b"{kind}" {{\n')


# (what is undone, which tests, file, anchor, replacement): the shape
# scripts/mutation_anchors.py reads, its last three the file and the edit.
MUTANTS = [
    # ---- JPEG ----
    (
        "a JPEG's Exif is kept whole, its GPS position, camera and time with it",
        RUST,
        STRIP,
        "                turned = sniff::orientation(&data[EXIF.len()..]);\n",
        "                turned = sniff::orientation(&data[EXIF.len()..]);\n"
        "                segment(&mut kept, m, data);\n",
    ),
    (
        "a JPEG's XMP (an APP1 not read for its turn) is kept",
        RUST,
        STRIP,
        JPEG_REST,
        "            0xE1 => segment(&mut kept, m, data),\n            _ => {}\n",
    ),
    (
        "a JPEG's IPTC (APP13) is kept",
        RUST,
        STRIP,
        JPEG_REST,
        "            0xED => segment(&mut kept, m, data),\n            _ => {}\n",
    ),
    (
        "a JPEG's comment (COM) is kept",
        RUST,
        STRIP,
        JPEG_REST,
        "            0xFE => segment(&mut kept, m, data),\n            _ => {}\n",
    ),
    (
        "a JPEG's APP0 is kept whole, its thumbnail with it",
        RUST,
        STRIP,
        JFIF_KEPT,
        "                    jfif = Some(data.to_vec());\n",
    ),
    (
        "a JPEG's JFIF units and densities are kept (a screen's 144 dpi)",
        RUST,
        STRIP,
        JFIF_KEPT,
        "                    jfif = Some([&data[..12], &[0, 0]].concat());\n",
    ),
    (
        "what follows a JPEG's EOI is kept (a phone's second image)",
        RUST,
        STRIP,
        "    out.extend([0xFF, 0xD9]);\n    Ok(out)\n",
        "    out.extend([0xFF, 0xD9]);\n    out.extend(&b[at..]);\n    Ok(out)\n",
    ),
    (
        "a photo's turn is not written back",
        BROWSER,
        STRIP,
        "    if let Some(o @ 2..=8) = turned {\n",
        "    if let Some(o @ 2..=8) = turned.filter(|_| false) {\n",
    ),
    (
        "an Exif after the first scan is read for the turn",
        RUST,
        STRIP,
        "            0xE1 if !scanned && turned.is_none() && data.starts_with(EXIF) => {\n",
        "            0xE1 if turned.is_none() && data.starts_with(EXIF) => {\n",
    ),
    (
        "the last Exif's turn is kept, not the first a browser reads",
        RUST,
        STRIP,
        "            0xE1 if !scanned && turned.is_none() && data.starts_with(EXIF) => {\n",
        "            0xE1 if !scanned && data.starts_with(EXIF) => {\n",
    ),
    (
        "a scan's entropy-coded data is not kept",
        RUST,
        STRIP,
        "                kept.extend(&b[at..end]);\n",
        "",
    ),
    (
        "a JPEG's quantization tables (DQT) are not kept",
        RUST,
        STRIP,
        "    matches!(m, 0xC0..=0xCF | 0xDB..=0xDF) && m != 0xC8\n",
        "    matches!(m, 0xC0..=0xCF | 0xDC..=0xDF) && m != 0xC8\n",
    ),
    (
        "a JPEG's ICC profile is not kept",
        RUST,
        STRIP,
        ICC_READ,
        "",
    ),
    (
        "a JPEG's ICC profile is kept as sent, what it names with it",
        RUST,
        STRIP,
        ICC_READ,
        "                    segment(&mut kept, m, data);\n",
    ),
    (
        "an ICC chunk after a JPEG's first scan is read",
        RUST,
        STRIP,
        "                if !scanned {\n" + ICC_READ,
        "                if !scanned || m == 0xE2 {\n" + ICC_READ,
    ),
    (
        "ICC chunks whose counts differ are read as one profile",
        RUST,
        STRIP,
        "        let numbered = n >= 1 && n <= count as usize && (self.count == 0 || self.count == count);\n",
        "        let numbered = n >= 1 && n <= count as usize;\n",
    ),
    (
        "an ICC chunk whose number repeats is read",
        RUST,
        STRIP,
        "        if !numbered || self.parts[n - 1].is_some() {\n",
        "        if !numbered {\n",
    ),
    (
        "a profile is split into chunks one byte longer than an APP2 holds",
        RUST,
        STRIP,
        "const ICC_PART: usize = 65_519;\n",
        "const ICC_PART: usize = 65_520;\n",
    ),
    (
        "a scan ends at a restart marker (every JPEG with restart intervals refused)",
        RUST,
        STRIP,
        "            Some(0x00 | 0xD0..=0xD7) => at = next + 1,\n",
        "            Some(0x00) => at = next + 1,\n",
    ),
    (
        "Adobe's APP14 is kept whole, past its twelve bytes",
        RUST,
        STRIP,
        "                segment(&mut kept, m, &data[..12]);\n",
        "                segment(&mut kept, m, data);\n",
    ),
    (
        "Adobe's APP14 (how the colours are coded) is not kept",
        RUST,
        STRIP,
        "            0xEE if data.starts_with(ADOBE) && data.len() >= 12 => {\n",
        "            0xEE if false => {\n",
    ),
    (
        "a JPEG that ends before its EOI is kept",
        RUST,
        STRIP,
        "        // A marker: `FF`, any fill bytes `FF`, and its code.\n",
        "        if at >= b.len() {\n            break;\n        }\n"
        "        // A marker: `FF`, any fill bytes `FF`, and its code.\n",
    ),
    # ---- PNG ----
    ("a PNG's tEXt is kept", RUST, STRIP, PNG_KEEPS, png_keeps("tEXt")),
    ("a PNG's zTXt is kept", RUST, STRIP, PNG_KEEPS, png_keeps("zTXt")),
    ("a PNG's iTXt (XMP) is kept", RUST, STRIP, PNG_KEEPS, png_keeps("iTXt")),
    ("a PNG's tIME is kept", RUST, STRIP, PNG_KEEPS, png_keeps("tIME")),
    ("a PNG's eXIf is kept", RUST, STRIP, PNG_KEEPS, png_keeps("eXIf")),
    ("a PNG's pHYs is kept", RUST, STRIP, PNG_KEEPS, png_keeps("pHYs")),
    ("a PNG's private chunk is kept", RUST, STRIP, PNG_KEEPS, png_keeps("prVt")),
    (
        "what follows a PNG's IEND is kept",
        RUST,
        STRIP,
        '        if kind == b"IEND" {\n            return Ok(out);\n',
        '        if kind == b"IEND" {\n            return Ok([out, b[at..].to_vec()].concat());\n',
    ),
    ("a PNG's palette (PLTE) is not kept", RUST, STRIP, PNG_KEEPS, png_drops("PLTE")),
    ("a PNG's transparency (tRNS) is not kept", RUST, STRIP, PNG_KEEPS, png_drops("tRNS")),
    ("an animated PNG's frame data (fdAT) is not kept", RUST, STRIP, PNG_KEEPS, png_drops("fdAT")),
    (
        "a PNG's ICC profile (iCCP) is not kept",
        RUST,
        STRIP,
        ICCP_KEPT,
        ICCP_KEPT.replace(") {\n", ").filter(|_| false) {\n"),
    ),
    (
        "a PNG's iCCP is kept as sent, its profile and its name with it",
        RUST,
        STRIP,
        ICCP_KEPT,
        ICCP_KEPT.replace(") {\n", ").map(|_| chunk[8..8 + length].to_vec()) {\n"),
    ),
    (
        "a PNG's iCCP keeps its name",
        RUST,
        STRIP,
        "    Some([icc::NEUTRAL, &[0, 0], &deflated].concat())\n",
        "    Some([&data[..nul], &[0, 0], &deflated].concat())\n",
    ),
    (
        "a PNG's second iCCP is kept",
        RUST,
        STRIP,
        "                profiled = true;\n" + ICCP_KEPT,
        ICCP_KEPT,
    ),
    (
        "a PNG's iCCP is inflated without a bound",
        RUST,
        STRIP,
        "    let profile = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(stream, icc::MAX).ok()?;\n",
        "    let profile = miniz_oxide::inflate::decompress_to_vec_zlib(stream).ok()?;\n",
    ),
    (
        "a PNG's iCCP compressed by another method is read",
        RUST,
        STRIP,
        "    if data.get(nul + 1) != Some(&0) {\n",
        "    if data.get(nul + 1).is_none() {\n",
    ),
    (
        "a PNG's iCCP without a name is read",
        RUST,
        STRIP,
        "        .filter(|&n| n > 0)?;\n",
        "        .filter(|_| true)?;\n",
    ),
    (
        "a PNG's iCCP whose name is past 79 bytes is read",
        RUST,
        STRIP,
        "        .take(80)\n",
        "        .take(81)\n",
    ),
    (
        "a critical chunk it does not know is dropped, not refused",
        RUST,
        STRIP,
        "        } else if kind[0].is_ascii_uppercase() {\n",
        "        } else if kind[0].is_ascii_uppercase() && false {\n",
    ),
    (
        "a PNG that ends before its IEND is kept",
        RUST,
        STRIP,
        "        let length = be32(b, at).ok_or(SHORT)? as usize;\n",
        "        let Some(length) = be32(b, at).map(|l| l as usize) else {\n"
        "            return Ok(out);\n"
        "        };\n",
    ),
    # ---- WebP ----
    (
        "a WebP's EXIF chunk is kept",
        RUST,
        STRIP,
        WEBP_KEEPS,
        WEBP_KEEPS.replace(" => {\n", ' | b"EXIF" => {\n'),
    ),
    (
        "a WebP's XMP chunk is kept",
        RUST,
        STRIP,
        WEBP_KEEPS,
        WEBP_KEEPS.replace(" => {\n", ' | b"XMP " => {\n'),
    ),
    (
        "a WebP's unknown chunk is kept",
        RUST,
        STRIP,
        "                    // EXIF, `XMP `, a second ICCP or VP8X, and chunks a\n"
        "                    // writer made up.\n"
        "                    _ => {}\n",
        '                    b"EXIF" | b"XMP " | b"ICCP" | b"VP8X" => {}\n'
        "                    _ => chunk(&mut kept, fourcc, data),\n",
    ),
    (
        "VP8X's Exif flag is left set",
        RUST,
        STRIP,
        "            header[0] &= !(VP8X_EXIF | VP8X_XMP);\n",
        "            header[0] &= !VP8X_XMP;\n",
    ),
    (
        "VP8X's XMP flag is left set",
        RUST,
        STRIP,
        "            header[0] &= !(VP8X_EXIF | VP8X_XMP);\n",
        "            header[0] &= !VP8X_EXIF;\n",
    ),
    (
        "an animated WebP frame's unknown chunk is kept",
        RUST,
        STRIP,
        '                            if matches!(fourcc, b"ALPH" | b"VP8 " | b"VP8L") {\n',
        "                            if !fourcc.is_empty() {\n",
    ),
    (
        "a WebP's ICC profile (ICCP) is not kept",
        RUST,
        STRIP,
        ICCP_SCRUBBED,
        ICCP_SCRUBBED.replace(") {\n", ").filter(|_| false) {\n"),
    ),
    (
        "a WebP's ICC profile is kept as sent, what it names with it",
        RUST,
        STRIP,
        ICCP_SCRUBBED,
        ICCP_SCRUBBED.replace(") {\n", ").map(|_| data.to_vec()) {\n"),
    ),
    (
        "a WebP's second ICCP is kept",
        RUST,
        STRIP,
        '                    b"ICCP" if !profiled => {\n',
        '                    b"ICCP" => {\n',
    ),
    (
        "VP8X flags an ICC profile where none is kept",
        RUST,
        STRIP,
        "                kept[8] &= !VP8X_ICC;\n",
        "",
    ),
    (
        "an animated WebP's ANIM is not kept",
        RUST,
        STRIP,
        WEBP_KEEPS,
        WEBP_KEEPS.replace('b"ANIM" | ', ""),
    ),
    (
        "a WebP's RIFF size is not rewritten",
        RUST,
        STRIP,
        "    out.extend(((kept.len() + 4) as u32).to_le_bytes());\n",
        "    out.extend(&b[4..8]);\n",
    ),
    (
        "a RIFF chunk of an odd size is not padded",
        RUST,
        STRIP,
        "    if data.len() % 2 == 1 {\n        out.push(0);\n    }\n",
        "",
    ),
    (
        "a WebP chunk past its RIFF is kept as far as it goes",
        RUST,
        STRIP,
        "            Some((b.get(..4)?, b.get(8..8 + size)?, size))\n",
        "            Some((b.get(..4)?, b.get(8..(8 + size).min(b.len()))?, size))\n",
    ),
    (
        "a WebP's chunks are listed before they are walked (four times its bytes)",
        RUST,
        STRIP,
        "    let mut found = Chunks(body);\n",
        "    let mut found = Chunks(body).collect::<Vec<_>>().into_iter();\n",
    ),
    # ---- GIF ----
    (
        "a GIF's comment is kept",
        RUST,
        STRIP,
        GIF_REST,
        "                    0xFE => out.extend(&b[at..end]),\n                    _ => {}\n",
    ),
    (
        "a GIF's plain text is kept",
        RUST,
        STRIP,
        GIF_REST,
        "                    0x01 => out.extend(&b[at..end]),\n                    _ => {}\n",
    ),
    (
        "another application's extension (XMP's) is kept",
        RUST,
        STRIP,
        LOOP_COUNT + "                        }\n",
        LOOP_COUNT
        + "                        } else {\n"
        + "                            out.extend(&b[at..end]);\n"
        + "                        }\n",
    ),
    (
        "a loop extension's other sub-blocks are kept",
        RUST,
        STRIP,
        LOOP_COUNT,
        "                            out.extend(&ext[12..]);\n",
    ),
    (
        "a graphic control's other sub-blocks are kept",
        RUST,
        STRIP,
        "                        out.extend(&ext[..5]);\n                        out.push(0);\n",
        "                        out.extend(ext);\n",
    ),
    (
        "what follows a GIF's trailer is kept",
        RUST,
        STRIP,
        "                out.push(0x3B);\n                return Ok(out);\n",
        "                out.extend(&b[at..]);\n                return Ok(out);\n",
    ),
    (
        "a GIF's loop count is not kept",
        RUST,
        STRIP,
        "                        if let Some(count) = looping(ext) {\n",
        "                        if let Some(count) = looping(ext).filter(|_| false) {\n",
    ),
    (
        "a GIF's graphic control (its timing and transparency) is not kept",
        RUST,
        STRIP,
        "                    0xF9 => {\n",
        "                    0xF9 if false => {\n",
    ),
    (
        "ANIMEXTS1.0 is not read as a loop",
        RUST,
        STRIP,
        '    if id != b"NETSCAPE2.0" && id != b"ANIMEXTS1.0" {\n',
        '    if id != b"NETSCAPE2.0" {\n',
    ),
    (
        "a GIF that ends before its trailer is kept",
        RUST,
        STRIP,
        "        match *b.get(at).ok_or(SHORT)? {\n",
        "        match b.get(at).copied().unwrap_or(0x3B) {\n",
    ),
    (
        "a GIF image's own colour table is not skipped (every GIF with per-frame palettes refused)",
        RUST,
        STRIP,
        "                let local = table(*b.get(at + 9).ok_or(SHORT)?);\n",
        "                let local = 0;\n",
    ),
    # ---- An ICC profile (uploads/icc.rs) ----
    (
        "an ICC profile's creation time is kept",
        RUST,
        ICC,
        ZEROED,
        "const ZEROED: [(usize, usize); 2] = [(48, 64), (84, 128)];\n",
    ),
    (
        "an ICC profile's device maker, model and attributes are kept",
        RUST,
        ICC,
        ZEROED,
        "const ZEROED: [(usize, usize); 2] = [(24, 36), (84, 128)];\n",
    ),
    (
        "an ICC profile's ID is kept",
        RUST,
        ICC,
        ZEROED,
        "const ZEROED: [(usize, usize); 3] = [(24, 36), (48, 64), (100, 128)];\n",
    ),
    (
        "an ICC profile's desc text is kept (a version 2 description, device maker or model)",
        RUST,
        ICC,
        NEUTRAL,
        '        b"desc" => Keeps::Whole,\n        b"mluc" | b"text" => Keeps::Neutral,\n',
    ),
    (
        "an ICC profile's mluc text is kept (a version 4 description, device maker or model)",
        RUST,
        ICC,
        NEUTRAL,
        '        b"mluc" => Keeps::Whole,\n        b"desc" | b"text" => Keeps::Neutral,\n',
    ),
    (
        "an ICC profile's text-typed text is kept (a version 2 copyright)",
        RUST,
        ICC,
        NEUTRAL,
        '        b"text" => Keeps::Whole,\n        b"desc" | b"mluc" => Keeps::Neutral,\n',
    ),
    (
        "an ICC tag of another type is kept (Apple's mmod with a serial number, a calibration's time)",
        RUST,
        ICC,
        "        _ => Keeps::Nothing,\n",
        "        _ => Keeps::Whole,\n",
    ),
    (
        "an ICC profile's XYZ tags (its white and primaries) are dropped",
        RUST,
        ICC,
        '        b"XYZ " | b"curv" | b"para" | b"sf32" | b"mft1" | b"mft2" | b"mAB " | b"mBA " | b"mpet"\n',
        '        b"curv" | b"para" | b"sf32" | b"mft1" | b"mft2" | b"mAB " | b"mBA " | b"mpet"\n',
    ),
    (
        "an ICC colour tag is not kept byte for byte (its last byte lost)",
        RUST,
        ICC,
        "            Keeps::Whole => out.extend(&p[at..at + len]),\n",
        "            Keeps::Whole => out.extend(&p[at..at + len - 1]),\n",
    ),
    (
        "two ICC tags whose data overlap are read",
        RUST,
        ICC,
        "    if ranges.windows(2).any(|w| w[0].0 + w[0].1 > w[1].0) {\n",
        "    if ranges.windows(2).any(|w| w[0] == w[1]) {\n",
    ),
    (
        "an ICC tag in its profile's table is read",
        RUST,
        ICC,
        TAG_READ,
        "        if len < 4 || at.checked_add(len)? > size {\n",
    ),
    (
        "an ICC tag shorter than its type's signature is read",
        RUST,
        ICC,
        TAG_READ,
        "        if at < table || at.checked_add(len)? > size {\n",
    ),
    (
        "an ICC profile shorter than its header says is read",
        RUST,
        ICC,
        "    if size < HEADER + 4 || size > profile.len() || size > MAX {\n",
        "    if size < HEADER + 4 || size > MAX {\n",
    ),
    (
        "an ICC profile without its acsp signature is read",
        RUST,
        ICC,
        SIGNED,
        "    if p[8] > 4 {\n",
    ),
    (
        "an ICC profile of a version over 4 (iccMAX's) is read",
        RUST,
        ICC,
        SIGNED,
        '    if &p[36..40] != b"acsp" {\n',
    ),
    (
        "an ICC profile of more than 1,024 tags is read",
        RUST,
        ICC,
        "    if count > MAX_TAGS {\n",
        "    if count > 2 * MAX_TAGS {\n",
    ),
    (
        "an ICC profile a fixed text makes larger than 4 MiB is kept (and not kept again)",
        RUST,
        ICC,
        "    if out.len() > MAX {\n",
        "    if out.len() > 2 * MAX {\n",
    ),
    # ---- The upload path ----
    (
        "the bytes as sent are leased, not the bytes rewritten",
        BROWSER,
        UPLOADS,
        "            &kept,\n            bytes.len() as u64,\n",
        "            bytes,\n            bytes.len() as u64,\n",
    ),
    (
        "a file the rewrite refuses is kept as sent",
        RUST,
        UPLOADS,
        "        let (kept, measured) = match strip::strip(bytes)\n",
        "        let (kept, measured) = match strip::strip(bytes)\n"
        "            .or_else(|_| Ok::<_, Unreadable>(bytes.to_vec()))\n",
    ),
    (
        "an image is measured as sent, not as kept",
        RUST,
        UPLOADS,
        "            .and_then(|kept| sniff::measure(&kept).map(|measured| (kept, measured)))\n",
        "            .and_then(|kept| sniff::measure(bytes).map(|measured| (kept, measured)))\n",
    ),
    (
        "a user's hourly bytes count what is kept, not what was sent",
        RUST,
        UPLOADS,
        "        let size = sent;\n",
        "        let size = bytes.len() as u64;\n",
    ),
    (
        "a lease replaced by the same bytes forgets them",
        RUST,
        UPLOADS,
        "        *state.holds.entry(key.clone()).or_default() += 1;\n"
        "        if let Some(old) = state.leases.remove(session) {\n"
        "            self.end(&mut state, old);\n"
        "        }\n",
        "        if let Some(old) = state.leases.remove(session) {\n"
        "            self.end(&mut state, old);\n"
        "        }\n"
        "        *state.holds.entry(key.clone()).or_default() += 1;\n",
    ),
]

SERVER = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "uploads", "blob"]
SERVER_BUILD = ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"]
FEED_BUILD = ["bash", "spikes/own-renderer/feed.sh"]
SPEC = ["pnpm", "exec", "playwright", "test", "e2e/upload-metadata.spec.mjs", "--reporter=line"]

# How long a browser run may take before it is stopped and counted failed.
BOUND = 900


def rust():
    """(built, passed, failed) of the server's tests."""
    r = subprocess.run(SERVER, cwd=ROOT, capture_output=True, text=True)
    results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", r.stdout + r.stderr)
    if not results:
        return False, 0, 0
    return True, sum(int(p) for p, _ in results), sum(int(f) for _, f in results)


def browser():
    """(built, passed, failed) of the browser suite, on a server built now."""
    if subprocess.run(SERVER_BUILD, cwd=ROOT, capture_output=True, text=True).returncode != 0:
        return False, 0, 0
    p = subprocess.Popen(
        SPEC,
        cwd=ROOT / "spikes/own-renderer",
        start_new_session=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"^\s+(\d+) passed", out, re.M))
    failed = sum(int(n) for n in re.findall(r"^\s+(\d+) (?:failed|did not run)", out, re.M))
    return passed + failed > 0, passed, failed


SUITES = {RUST: rust, BROWSER: browser}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline

    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    feed = subprocess.run(FEED_BUILD, cwd=ROOT, capture_output=True, text=True)
    if feed.returncode != 0:
        print("FAIL: the feed did not build; no browser mutant can mean anything")
        print(feed.stdout[-2000:] + feed.stderr[-2000:])
        return 1
    for suite, run in SUITES.items():
        built, passed, failed = run()
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = SUITES[suite]()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
