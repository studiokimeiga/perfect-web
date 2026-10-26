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

import cargo_assertion_results
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
STRIP = ROOT / "spikes/own-renderer/server/src/uploads/strip.rs"
ICC = ROOT / "spikes/own-renderer/server/src/uploads/icc.rs"
UPLOADS = ROOT / "spikes/own-renderer/server/src/uploads.rs"

RUST = "rust"
BROWSER = "browser"

# The catch-alls a mutant adds an arm before.
JPEG_REST = "            // Every other APPn, COM, JPGn and RESn.\n            _ => {}\n"
PNG_KEEPS = "        if PNG_KEPT.iter().any(|k| &k[..] == kind) {\n"
WEBP_KEEPS = '                    b"VP8 " | b"VP8L" => {\n'
JFIF_KEPT = "                    jfif = Some([&data[..7], &[0, 0, 1, 0, 1, 0, 0]].concat());\n"
ICC_READ = "                    profile.add(data);\n"
ICCP_KEPT = "                if let Some(data) = iccp(&chunk[8..8 + length]) {\n"
ICCP_SCRUBBED = "                        if let Some(profile) = icc::scrub(data) {\n"
ZEROED = "const ZEROED: [(usize, usize); 6] = [(4, 8), (10, 12), (24, 36), (40, 44), (48, 64), (80, 128)];\n"
NEUTRAL = '        b"desc" | b"mluc" | b"text" if text_tag(sig) => Keeps::Neutral,\n'
TAG_READ = "        if at < table || len < 8 || at.checked_add(len)? > size {\n"
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
    # ---- The integrator's additional review cases ----
    ("rewritten image growth is omitted from the staging budget", RUST, UPLOADS,
     "        let staged_size = size.max(bytes.len() as u64);\n", "        let staged_size = size;\n"),
    ("an expanded image releases only its input-byte charge", RUST, UPLOADS,
     "                bytes: staged_size,\n", "                bytes: size,\n"),
    ("ICC unknown text tag signatures are kept", RUST, ICC,
     '        b"desc" | b"mluc" | b"text" if text_tag(sig) => Keeps::Neutral,\n',
     '        b"desc" | b"mluc" | b"text" => Keeps::Neutral,\n'),
    ("Adobe producer version bytes are kept", RUST, STRIP,
     "                adobe[5..7].copy_from_slice(&100u16.to_be_bytes());\n", ""),
    ("ICC colour payload tails are accepted", RUST, ICC,
     "    if data.len() != length {\n", "    if data.len() < length {\n"),
    ("ICC shared transforms ignore distinct matrix contexts", RUST, ICC,
     "                    if different_spaces {\n", "                    if false {\n"),
    ("ICC header colour identifiers are unchecked", RUST, ICC,
     "    if !header_is_colour(p) {\n", "    if false {\n"),
    ("ICC unused legacy LUT matrix is kept", RUST, ICC,
     "        if input_space != b\"XYZ \" {\n", "        if false {\n"),
    ("ICC type-specific reserved fields and unused matrix normalization are bypassed", RUST, ICC,
     "                out.extend(colour_tag(&tag.sig, &p[at..at + len], input_space)?);\n",
     "                out.extend(&p[at..at + len]);\n"),
    ("ICC unknown colour tag signatures are admitted", RUST, ICC,
     "            if known {\n", "            if known || !sig.is_empty() {\n"),
    ("ICC parametric-curve reserved bytes are kept", RUST, ICC,
     "        out[10..12].fill(0);\n", ""),
    ("ICC legacy LUT reserved byte is kept", RUST, ICC,
     "        out[11] = 0;\n", ""),
    ("PNG CRC bytes are copied from the sender", RUST, STRIP,
     "            out.extend(png_chunk(kind, &chunk[8..8 + length]));\n", "            out.extend(chunk);\n"),
    ("PNG transparency length is unchecked", RUST, STRIP,
     "        b\"tRNS\" => {\n", "        b\"tRNS\" => return true,\n        b\"ZZZZ\" => {\n"),
    ("WebP ALPH reserved bits are kept", RUST, STRIP,
     "    *packed &= 0x3f;\n", "    *packed &= 0xff;\n"),
    ("WebP ANMF reserved bits are kept", RUST, STRIP,
     "                        frame[15] &= 3;\n", "                        frame[15] &= 0xff;\n"),
    ("WebP ANIM retains excess bytes", RUST, STRIP,
     "                        if data.len() != 6 {\n", "                        if false {\n"),
    ("GIF descriptor reserved bits are kept", RUST, STRIP,
     "                out[start + 9] &= 0xe7;\n", "                out[start + 9] &= 0xff;\n"),
    ("GIF graphic-control reserved bits are kept", RUST, STRIP,
     "                        out.extend([4, ext[1] & 0x1f, ext[2], ext[3], ext[4]]);\n",
     "                        out.extend([4, ext[1], ext[2], ext[3], ext[4]]);\n"),
    ("ICC undefined rendering intents are kept", RUST, ICC,
     "    if be32(p, 64)? > 3 {\n", "    if false {\n"),
    ("PNG fixed chunks keep trailing payloads", RUST, STRIP,
     "        if !png_length(kind, length, colour, palette) {\n", "        if false {\n"),
    ("Adobe APP14 producer flags are kept", RUST, STRIP,
     "                adobe[7..11].fill(0);\n", ""),
    ("WebP VP8X reserved flag bits are kept", RUST, STRIP,
     "            header[0] &= 0x3e;\n", ""),
    ("WebP VP8X reserved bytes are kept", RUST, STRIP,
     "            header[1..4].fill(0);\n", ""),
    ("an ICC profile's version reserved bytes are kept", RUST, ICC, ZEROED,
     "const ZEROED: [(usize, usize); 5] = [(4, 8), (24, 36), (40, 44), (48, 64), (80, 128)];\n"),
    ("an ICC profile's tag reserved bytes are kept", RUST, ICC,
     "                out[start + 4..start + 8].fill(0);\n", ""),
    ("an ICC profile's vendor flags are kept", RUST, ICC,
     "    out[44..48].copy_from_slice(&(be32(p, 44)? & 3).to_be_bytes());\n",
     "    out[44..48].copy_from_slice(&be32(p, 44)?.to_be_bytes());\n"),
    ("an ICC profile's preferred CMM signature is kept", RUST, ICC, ZEROED,
     "const ZEROED: [(usize, usize); 5] = [(10, 12), (24, 36), (40, 44), (48, 64), (80, 128)];\n"),
    ("an ICC profile's platform signature is kept", RUST, ICC, ZEROED,
     "const ZEROED: [(usize, usize); 5] = [(4, 8), (10, 12), (24, 36), (48, 64), (80, 128)];\n"),
    ("an ICC profile's creator signature is kept", RUST, ICC, ZEROED,
     "const ZEROED: [(usize, usize); 6] = [(4, 8), (10, 12), (24, 36), (40, 44), (48, 64), (84, 128)];\n"),
    ("JPEG orientation 2 is lost", RUST, STRIP,
     "    if let Some(o @ 2..=8) = turned {\n", "    if let Some(o @ 3..=8) = turned {\n"),
    ("JPEG orientation 8 is lost", RUST, STRIP,
     "    if let Some(o @ 2..=8) = turned {\n", "    if let Some(o @ 2..=7) = turned {\n"),
    ("JPEG segment fill is skipped only once", RUST, STRIP,
     "        while b.get(at) == Some(&0xFF) {\n", "        if b.get(at) == Some(&0xFF) {\n"),
    ("JPEG scan fill is skipped only once", RUST, STRIP,
     "        while b.get(next) == Some(&0xFF) {\n", "        if b.get(next) == Some(&0xFF) {\n"),
    ("short JFIF data is sliced", RUST, STRIP,
     "            0xE0 if data.starts_with(JFIF) && data.len() >= 14 => {\n",
     "            0xE0 if data.starts_with(JFIF) => {\n"),
    ("short Adobe data is sliced", RUST, STRIP,
     "            0xEE if data.starts_with(ADOBE) && data.len() >= 12 => {\n",
     "            0xEE if data.starts_with(ADOBE) => {\n"),
    ("a PNG's suggested palette is kept", RUST, STRIP, PNG_KEEPS, png_keeps("sPLT")),
    ("a PNG's histogram is kept", RUST, STRIP, PNG_KEEPS, png_keeps("hIST")),
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
        "                segment(&mut kept, m, &adobe);\n",
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
        '                            } else if matches!(fourcc, b"VP8 " | b"VP8L") {\n',
        "                            } else if !fourcc.is_empty() {\n",
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
        '                    b"ANIM" => {\n',
        '                    b"ANIM" if false => {\n',
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
        "                        out.extend([4, ext[1] & 0x1f, ext[2], ext[3], ext[4]]);\n                        out.push(0);\n",
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
        "const ZEROED: [(usize, usize); 5] = [(4, 8), (10, 12), (40, 44), (48, 64), (80, 128)];\n",
    ),
    (
        "an ICC profile's device maker, model and attributes are kept",
        RUST,
        ICC,
        ZEROED,
        "const ZEROED: [(usize, usize); 5] = [(4, 8), (10, 12), (24, 36), (40, 44), (80, 128)];\n",
    ),
    (
        "an ICC profile's ID is kept",
        RUST,
        ICC,
        ZEROED,
        "const ZEROED: [(usize, usize); 7] = [(4, 8), (10, 12), (24, 36), (40, 44), (48, 64), (80, 84), (100, 128)];\n",
    ),
    (
        "an ICC desc tag is admitted as colour instead of neutral text",
        RUST,
        ICC,
        NEUTRAL,
        '        b"desc" => Keeps::Whole,\n        b"mluc" | b"text" => Keeps::Neutral,\n',
    ),
    (
        "an ICC mluc tag is admitted as colour instead of neutral text",
        RUST,
        ICC,
        NEUTRAL,
        '        b"mluc" => Keeps::Whole,\n        b"desc" | b"text" => Keeps::Neutral,\n',
    ),
    (
        "an ICC text tag is admitted as colour instead of neutral text",
        RUST,
        ICC,
        NEUTRAL,
        '        b"text" => Keeps::Whole,\n        b"desc" | b"mluc" => Keeps::Neutral,\n',
    ),
    (
        "an unsupported ICC tag type is admitted as colour",
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
        "                out.extend(colour_tag(&tag.sig, &p[at..at + len], input_space)?);\n",
        "                out.extend(colour_tag(&tag.sig, &p[at..at + len - 1], input_space)?);\n",
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
        "        if len < 8 || at.checked_add(len)? > size {\n",
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

SERVER = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "uploads", "blob", "--format=pretty"]
SERVER_BUILD = ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"]
FEED_BUILD = ["bash", "spikes/own-renderer/feed.sh"]
SPEC = ["pnpm", "exec", "playwright", "test", "e2e/upload-metadata.spec.mjs", "--reporter=line"]

# How long a browser run may take before it is stopped and counted failed.
BOUND = 900


RUST_SOURCES = [STRIP, ICC, UPLOADS, ROOT / "spikes/own-renderer/server/src/tests/uploads.rs", ROOT / "spikes/own-renderer/server/src/blob.rs"]
RUST_INVENTORY = None


def rust():
    """A complete named inventory, with failures at native assertion sites."""
    global RUST_INVENTORY
    try:
        child = producer_process.run(SERVER, cwd=ROOT, timeout=900)
    except subprocess.TimeoutExpired:
        return False, 0, 0
    result = cargo_assertion_results.validate(
        child.returncode, child.stdout, root=ROOT, sources=RUST_SOURCES,
        expected=RUST_INVENTORY,
    )
    if not result.valid:
        print(child.stdout[-4000:], flush=True)
        return False, result.passed, result.failed
    if RUST_INVENTORY is None:
        RUST_INVENTORY = cargo_assertion_results.names(child.stdout)
    return True, result.passed, result.failed


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
        return False, 0, 0
    return browser_result(out, p.returncode)


def browser_result(out, status):
    """Only completed, assertion-caused test failures can kill a mutant."""
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"^\s+(\d+) passed", out, re.M))
    failed = sum(int(n) for n in re.findall(r"^\s+(\d+) failed", out, re.M))
    infrastructure = re.search(
        r"did not run|interrupted|worker process exited|browserType\.launch|"
        r"Test timeout of|ECONNREFUSED|EADDRINUSE|Protocol error|"
        r"Target page, context or browser has been closed|webServer.*(?:failed|timed out)",
        out,
        re.I,
    )
    assertion = not failed or re.search(r"Error: expect\(|AssertionError|^\s*Expected:", out, re.M)
    valid = passed + failed > 0 and not infrastructure and bool(assertion) and (status != 0) == bool(failed)
    return bool(valid), passed, failed



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
            verdict = "INVALID (build, infrastructure or test-result failure)"
            survivors += 1
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
