//! **An image kept without what names a place, a device, a person or a
//! time** (track `upload-metadata`, ADR-0383, amending ADR-0260).
//!
//! ADR-0260 kept and served an upload's bytes as they were sent, so its
//! metadata went with them: a photo taken on a phone and posted in the feed
//! published where it was taken (Exif's GPS IFD, XMP), the camera, when, and
//! whatever its XMP, IPTC or a comment named. Every upload is now rewritten
//! by the host before it is kept: its format's container walked to its end,
//! what decoding, colour and a photo's turn need copied whole and in its
//! order, and the rest dropped. A rewriter, not a decoder: no pixel is read
//! or changed. A file whose container does not parse is refused whole, so
//! nothing half-rewritten is ever kept; what is kept is rewritten to itself;
//! and it is what is served.
//!
//! - **JPEG** (ITU-T T.81, Annex B; JFIF 1.02; Exif's APP1 laid out as TIFF
//!   6.0's structure): kept, the frame and scan headers, the tables (DQT,
//!   DHT, DAC), DRI, DNL, the hierarchical process's DHP and EXP, and each
//!   scan's entropy-coded data, byte for byte; JFIF's APP0 without its
//!   thumbnail; APP2's ICC profile; Adobe's APP14, its twelve bytes, which
//!   say how the colours are coded. A browser turns a photo by its Exif
//!   `Orientation` (CSS Images 3, `image-orientation: from-image`), so one of
//!   2 to 8 is written back as an APP1 that holds that one tag. Dropped:
//!   every other APPn (Exif's whole APP1 with its thumbnail and maker notes,
//!   XMP, IPTC's APP13, MPF, C2PA's APP11, ...), COM, reserved markers, and
//!   whatever follows EOI (a phone's second image, or its video).
//! - **PNG** (W3C PNG, third edition, §4.8.2): kept, the four critical
//!   chunks, tRNS, the colour chunks a browser renders by (cHRM, gAMA, iCCP,
//!   sRGB, cICP, mDCV, cLLI) and animation's (acTL, fcTL, fdAT). Every other
//!   ancillary chunk is dropped (tEXt, zTXt, iTXt, tIME, eXIf, pHYs, sBIT,
//!   bKGD, hIST, sPLT, and any a writer made up), and whatever follows IEND.
//!   A critical chunk this does not know is refused: it cannot be dropped
//!   without changing what the image is (§5.4, "information it cannot
//!   safely interpret").
//! - **WebP** (RFC 9649): a simple file is its one image chunk; an extended
//!   one keeps VP8X, its Exif and XMP flags cleared, then ICCP, ANIM, ANMF
//!   (each frame's unknown chunks dropped), ALPH, `VP8 ` and VP8L. Dropped:
//!   EXIF, `XMP `, unknown chunks, and whatever follows the RIFF's size.
//! - **GIF** (GIF89a): kept, the header, the logical screen and its colour
//!   table, each image whole, each graphic control extension's four bytes,
//!   and a looping extension's loop count (Netscape's `NETSCAPE2.0`, and
//!   `ANIMEXTS1.0`, its other name). Dropped: comments, plain text, every
//!   other application's extension (XMP's among them), and whatever follows
//!   the trailer.

use super::icc;
use super::sniff::{self, Kind, Unreadable, be16, be32, le32};

/// **What is kept of `bytes`**: the image, rewritten without its metadata,
/// or why its container does not parse as the kind its signature names.
pub fn strip(bytes: &[u8]) -> Result<Vec<u8>, Unreadable> {
    let kind = sniff::sniff(bytes).ok_or(Unreadable::Unrecognized)?;
    let kept = match kind {
        Kind::Jpeg => jpeg(bytes),
        Kind::Png => png(bytes),
        Kind::Webp => webp(bytes),
        Kind::Gif => gif(bytes),
    };
    kept.map_err(|why| Unreadable::Malformed(kind, why))
}

// ---- JPEG ----

/// The identifiers of the APPn segments a JPEG keeps (JFIF 1.02; ICC.1,
/// Annex B; Adobe's APP14), and of Exif's APP1.
const JFIF: &[u8] = b"JFIF\0";
const ICC: &[u8] = b"ICC_PROFILE\0";
const ADOBE: &[u8] = b"Adobe";
const EXIF: &[u8] = b"Exif\0\0";

/// **The most of a profile an APP2 chunk holds**: an APP2 segment's 65,533
/// bytes of data (its length's sixteen bits, less their own two), less
/// `ICC_PROFILE\0`, its number and its count (ICC.1, B.4; libjpeg-turbo's
/// `MAX_DATA_BYTES_IN_MARKER`).
const ICC_PART: usize = 65_519;

/// **An ICC profile's APP2 chunks, as libjpeg reads them**
/// (`jpeg_read_icc_profile`, libjpeg-turbo's `jdicc.c`, which the engines'
/// decoders call): each numbered from 1 to the count each gives, the same
/// count in all, no number twice and none missing; otherwise none is read.
struct IccChunks<'a> {
    count: u8,
    parts: [Option<&'a [u8]>; 255],
    bogus: bool,
}

impl<'a> IccChunks<'a> {
    fn new() -> IccChunks<'a> {
        IccChunks {
            count: 0,
            parts: [None; 255],
            bogus: false,
        }
    }

    /// A chunk's data: `ICC_PROFILE\0`, its number, the count, its part.
    fn add(&mut self, data: &'a [u8]) {
        let (n, count) = (data[ICC.len()] as usize, data[ICC.len() + 1]);
        let numbered = n >= 1 && n <= count as usize && (self.count == 0 || self.count == count);
        if !numbered || self.parts[n - 1].is_some() {
            self.bogus = true;
            return;
        }
        self.count = count;
        self.parts[n - 1] = Some(&data[ICC.len() + 2..]);
    }

    /// The profile they make, joined in their numbers' order.
    fn profile(&self) -> Option<Vec<u8>> {
        if self.bogus {
            return None;
        }
        let parts: Option<Vec<&[u8]>> = self.parts[..self.count as usize].iter().copied().collect();
        parts.map(|p| p.concat()).filter(|p| !p.is_empty())
    }
}

/// **A marker segment kept whole** (T.81, Table B.1): a frame header
/// (SOFn), a table (DHT `C4`, DAC `CC`, DQT `DB`), DNL, DRI, and the
/// hierarchical process's DHP and EXP. Numbers that decoding reads, and
/// nothing else. `C8` (JPG) is reserved, not a frame.
fn decodes(m: u8) -> bool {
    matches!(m, 0xC0..=0xCF | 0xDB..=0xDF) && m != 0xC8
}

/// A JPEG marker segment: `FF`, its code, its length (its data's and the
/// length's own two bytes), and its data.
fn segment(out: &mut Vec<u8>, m: u8, data: &[u8]) {
    out.extend([0xFF, m]);
    out.extend(((data.len() + 2) as u16).to_be_bytes());
    out.extend(data);
}

/// **A JPEG, its segments walked from SOI to EOI**: what decoding and colour
/// need copied in order, scans byte for byte, JFIF's APP0 without its
/// thumbnail, and a photo's turn written back alone.
fn jpeg(b: &[u8]) -> Result<Vec<u8>, &'static str> {
    const SHORT: &str = "it ends before its EOI";
    let mut jfif: Option<Vec<u8>> = None;
    let mut turned: Option<u32> = None;
    let mut profile = IccChunks::new();
    let mut scanned = false;
    let mut kept = Vec::with_capacity(b.len());
    let mut at = 2; // past SOI
    loop {
        // A marker: `FF`, any fill bytes `FF`, and its code.
        if b.get(at) != Some(&0xFF) {
            return Err(if at >= b.len() {
                SHORT
            } else {
                "a segment is not followed by a marker"
            });
        }
        while b.get(at) == Some(&0xFF) {
            at += 1;
        }
        let m = *b.get(at).ok_or(SHORT)?;
        at += 1;
        match m {
            0xD9 => break,
            0xD8 => return Err("a second SOI"),
            0x00 => return Err("a stuffed zero where a marker is"),
            // TEM and RSTm stand alone, and outside a scan say nothing.
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        let length = be16(b, at).ok_or(SHORT)? as usize;
        if length < 2 {
            return Err("a segment's length is under 2");
        }
        let data = b.get(at + 2..at + length).ok_or(SHORT)?;
        at += length;
        match m {
            // Its version; no units, and a density of 1 by 1, the pixels'
            // aspect alone (a density names the screen or the scanner that
            // made it, as PNG's pHYs does); and no thumbnail, which is the
            // picture as it was before an edit cropped it.
            0xE0 if data.starts_with(JFIF) && data.len() >= 14 => {
                if jfif.is_none() {
                    jfif = Some([&data[..7], &[0, 0, 1, 0, 1, 0, 0]].concat());
                }
            }
            // The turn written back: the first Exif structure whose
            // `Orientation` reads, before any scan. (Measured in Playwright
            // 1.58: of a JPEG's APP1 segments, Firefox reads only the first,
            // of any kind, and Chromium and WebKit the first Exif; what is
            // kept has one APP1, the turn alone, so all three show it
            // alike.)
            0xE1 if !scanned && turned.is_none() && data.starts_with(EXIF) => {
                turned = sniff::orientation(&data[EXIF.len()..]);
            }
            // An ICC profile's chunks, read as a browser reads them: before
            // any scan, and each long enough to be numbered.
            0xE2 if data.len() >= ICC.len() + 2 && data.starts_with(ICC) => {
                if !scanned {
                    profile.add(data);
                }
            }
            0xEE if data.starts_with(ADOBE) && data.len() >= 12 => {
                let mut adobe = data[..12].to_vec();
                adobe[5..7].copy_from_slice(&100u16.to_be_bytes());
                adobe[7..11].fill(0);
                segment(&mut kept, m, &adobe);
            }
            0xDA => {
                segment(&mut kept, m, data);
                let end = scan(b, at)?;
                kept.extend(&b[at..end]);
                at = end;
                scanned = true;
            }
            m if decodes(m) => segment(&mut kept, m, data),
            // Every other APPn, COM, JPGn and RESn.
            _ => {}
        }
    }
    let mut out = Vec::with_capacity(kept.len() + 64);
    out.extend([0xFF, 0xD8]);
    if let Some(jfif) = jfif {
        segment(&mut out, 0xE0, &jfif);
    }
    if let Some(o @ 2..=8) = turned {
        segment(&mut out, 0xE1, &orientation_alone(o as u16));
    }
    // The profile rewritten (`icc.rs`), in as few chunks as hold it.
    if let Some(profile) = profile.profile().and_then(|p| icc::scrub(&p)) {
        let parts = profile.chunks(ICC_PART);
        let count = parts.len() as u8;
        for (part, n) in parts.zip(1u8..) {
            segment(&mut out, 0xE2, &[ICC, &[n, count], part].concat());
        }
    }
    out.extend(kept);
    out.extend([0xFF, 0xD9]);
    Ok(out)
}

/// **Where a scan's entropy-coded data ends** (T.81, B.1.1.5): at its first
/// marker that is neither a stuffed `FF 00` nor a restart (`RSTm`), any fill
/// bytes before that marker not its own.
fn scan(b: &[u8], mut at: usize) -> Result<usize, &'static str> {
    const SHORT: &str = "it ends before its EOI";
    loop {
        let ff = at + b[at..].iter().position(|&x| x == 0xFF).ok_or(SHORT)?;
        let mut next = ff + 1;
        while b.get(next) == Some(&0xFF) {
            next += 1;
        }
        match b.get(next) {
            None => return Err(SHORT),
            Some(0x00 | 0xD0..=0xD7) => at = next + 1,
            Some(_) => return Ok(ff),
        }
    }
}

/// **An Exif APP1 that holds `Orientation` alone** (TIFF 6.0, §2): a
/// big-endian header, IFD0 of one entry (tag `0112`, SHORT, count 1, its
/// value left-justified in the entry), and no next IFD.
pub(crate) fn orientation_alone(o: u16) -> Vec<u8> {
    let mut d = EXIF.to_vec();
    d.extend(b"MM\0\x2A");
    d.extend(8u32.to_be_bytes());
    d.extend(1u16.to_be_bytes());
    d.extend(0x0112u16.to_be_bytes());
    d.extend(3u16.to_be_bytes());
    d.extend(1u32.to_be_bytes());
    d.extend(o.to_be_bytes());
    d.extend([0, 0]);
    d.extend(0u32.to_be_bytes());
    d
}

// ---- PNG ----

/// **The chunks a PNG keeps whole** (third edition, §4.8.2 and §11.3): the
/// critical four; transparency; colour space information a browser renders
/// by (sBIT, which none does, is not among them; iCCP is kept rewritten);
/// and animation's.
const PNG_KEPT: [&[u8; 4]; 14] = [
    b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"cHRM", b"gAMA", b"sRGB", b"cICP", b"mDCV",
    b"cLLI", b"acTL", b"fcTL", b"fdAT",
];

/// **An iCCP chunk's data, its profile rewritten** (third edition,
/// §11.3.2.4: a profile name of 1 to 79 bytes and its NUL, compression
/// method 0, then the profile in a zlib stream): its name, which is free
/// text (a person's, say), made [`icc::NEUTRAL`]; its profile inflated (to
/// at most [`icc::MAX`] bytes, with `miniz_oxide`, which this crate already
/// takes), rewritten (`icc.rs`) and deflated again. Nothing where any of it
/// does not read.
fn iccp(data: &[u8]) -> Option<Vec<u8>> {
    let nul = data
        .iter()
        .take(80)
        .position(|&c| c == 0)
        .filter(|&n| n > 0)?;
    if data.get(nul + 1) != Some(&0) {
        return None;
    }
    let stream = &data[nul + 2..];
    let profile = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(stream, icc::MAX).ok()?;
    let kept = icc::scrub(&profile)?;
    let deflated = miniz_oxide::deflate::compress_to_vec_zlib(&kept, 6);
    Some([icc::NEUTRAL, &[0, 0], &deflated].concat())
}

/// A PNG chunk: its data's length, its type, its data, and its CRC over its
/// type and data (§5.3).
fn png_chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
    let mut c = (data.len() as u32).to_be_bytes().to_vec();
    c.extend(kind);
    c.extend(data);
    c.extend(sniff::crc32(&c[4..]).to_be_bytes());
    c
}

/// Fixed-size PNG chunks cannot carry bytes outside their defined fields
/// (PNG third edition, sections 11.2, 11.3.2 and 11.3.6).
fn png_length(kind: &[u8], length: usize, colour: u8, palette: usize) -> bool {
    let fixed = match kind {
        b"IHDR" => 13,
        b"IEND" => 0,
        b"cHRM" => 32,
        b"gAMA" | b"cICP" => 4,
        b"sRGB" => 1,
        b"mDCV" => 24,
        b"cLLI" | b"acTL" => 8,
        b"fcTL" => 26,
        b"PLTE" => return length > 0 && length <= 768 && length.is_multiple_of(3),
        b"fdAT" => return length >= 4,
        b"tRNS" => {
            return match colour {
                0 => length == 2,
                2 => length == 6,
                3 => length > 0 && length <= palette,
                _ => false,
            };
        }
        _ => return true,
    };
    length == fixed
}

/// **A PNG, its chunks walked to IEND**: those it keeps copied whole, with
/// their CRCs, in order; its first iCCP rewritten, and its CRC computed.
fn png(b: &[u8]) -> Result<Vec<u8>, &'static str> {
    const SHORT: &str = "it ends before its IEND chunk";
    let mut out = b.get(..8).ok_or(SHORT)?.to_vec();
    let mut profiled = false;
    let colour = *b.get(25).ok_or(SHORT)?;
    let mut palette = 0;
    let mut at = 8;
    loop {
        let length = be32(b, at).ok_or(SHORT)? as usize;
        let kind = b.get(at + 4..at + 8).ok_or(SHORT)?;
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err("a chunk's type is not four letters");
        }
        if !png_length(kind, length, colour, palette) {
            return Err("a chunk has bytes outside its declared fields");
        }
        let end = (at + 12).checked_add(length).ok_or(SHORT)?;
        let chunk = b.get(at..end).ok_or(SHORT)?;
        if PNG_KEPT.iter().any(|k| &k[..] == kind) {
            if kind == b"PLTE" {
                palette = length / 3;
            }
            out.extend(png_chunk(kind, &chunk[8..8 + length]));
        } else if kind == b"iCCP" {
            // A PNG has one; a second is not read, so not kept.
            if !profiled {
                profiled = true;
                if let Some(data) = iccp(&chunk[8..8 + length]) {
                    out.extend(png_chunk(b"iCCP", &data));
                }
            }
        } else if kind[0].is_ascii_uppercase() {
            return Err("it has a critical chunk this does not know");
        }
        at = end;
        if kind == b"IEND" {
            return Ok(out);
        }
    }
}

// ---- WebP ----

/// VP8X's flags (RFC 9649, §2.7.1, its bits numbered from the most
/// significant): an ICC profile (I), Exif metadata (E) and XMP metadata
/// (X).
const VP8X_ICC: u8 = 0x20;
const VP8X_EXIF: u8 = 0x08;
const VP8X_XMP: u8 = 0x04;

/// A RIFF chunk: its FourCC, its size, its data, and a zero after an odd
/// size (RFC 9649, §2.3).
fn chunk(out: &mut Vec<u8>, fourcc: &[u8], data: &[u8]) {
    out.extend(fourcc);
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

/// A RIFF chunk read: its FourCC and its data.
type Chunk<'a> = (&'a [u8], &'a [u8]);

/// **A RIFF list's chunks, read one at a time** (the independent review,
/// finding 4): nothing is kept of a chunk once it has been read, so a file
/// of many empty chunks costs a walk, and no list of them. A last chunk of
/// an odd size may lack its padding. After a chunk longer than the list,
/// nothing more is read.
struct Chunks<'a>(&'a [u8]);

impl<'a> Iterator for Chunks<'a> {
    type Item = Result<Chunk<'a>, &'static str>;

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.0;
        if b.is_empty() {
            return None;
        }
        let read = || {
            let size = le32(b, 4)? as usize;
            Some((b.get(..4)?, b.get(8..8 + size)?, size))
        };
        let Some((fourcc, data, size)) = read() else {
            self.0 = &[];
            return Some(Err("a chunk is longer than its RIFF"));
        };
        self.0 = b.get(8 + size + size % 2..).unwrap_or(&[]);
        Some(Ok((fourcc, data)))
    }
}

/// ALPH's two high bits are reserved; the rest describes the coded alpha.
fn alpha(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut out = data.to_vec();
    let packed = out.first_mut().ok_or("an ALPH chunk is empty")?;
    *packed &= 0x3f;
    Ok(out)
}

/// **A WebP, its chunks walked to its RIFF's size**: a simple file's image,
/// or an extended file's image, colour and animation.
fn webp(b: &[u8]) -> Result<Vec<u8>, &'static str> {
    let size = le32(b, 4).ok_or("it ends before its RIFF header does")? as usize;
    let body = b
        .get(12..size.saturating_add(8))
        .ok_or("its RIFF size is not the bytes it has")?;
    let mut found = Chunks(body);
    let mut kept = Vec::new();
    match found.next().transpose()? {
        Some((first @ (b"VP8 " | b"VP8L"), data)) => {
            chunk(&mut kept, first, data);
            // The rest walked to the RIFF's end, and dropped.
            for walked in found {
                walked?;
            }
        }
        Some((b"VP8X", data)) => {
            let mut header = data.get(..10).ok_or("its VP8X chunk is short")?.to_vec();
            header[0] &= !(VP8X_EXIF | VP8X_XMP);
            header[0] &= 0x3e;
            header[1..4].fill(0);
            chunk(&mut kept, b"VP8X", &header);
            let (mut profiled, mut profile_kept) = (false, false);
            for walked in found {
                let (fourcc, data) = walked?;
                match fourcc {
                    // Its profile rewritten (`icc.rs`); a file has one, and
                    // a second is not read, so not kept.
                    b"ICCP" if !profiled => {
                        profiled = true;
                        if let Some(profile) = icc::scrub(data) {
                            chunk(&mut kept, fourcc, &profile);
                            profile_kept = true;
                        }
                    }
                    b"ANIM" => {
                        if data.len() != 6 {
                            return Err("an ANIM chunk is not six bytes");
                        }
                        chunk(&mut kept, fourcc, data);
                    }
                    b"ALPH" => chunk(&mut kept, fourcc, &alpha(data)?),
                    b"VP8 " | b"VP8L" => {
                        chunk(&mut kept, fourcc, data);
                    }
                    b"ANMF" => {
                        let mut frame = data
                            .get(..16)
                            .ok_or("an ANMF chunk is shorter than its frame header")?
                            .to_vec();
                        frame[15] &= 3;
                        for walked in Chunks(&data[16..]) {
                            let (fourcc, data) = walked?;
                            if fourcc == b"ALPH" {
                                chunk(&mut frame, fourcc, &alpha(data)?);
                            } else if matches!(fourcc, b"VP8 " | b"VP8L") {
                                chunk(&mut frame, fourcc, data);
                            }
                        }
                        chunk(&mut kept, b"ANMF", &frame);
                    }
                    // EXIF, `XMP `, a second ICCP or VP8X, and chunks a
                    // writer made up.
                    _ => {}
                }
            }
            // VP8X flags a profile where one is kept, and only there.
            if !profile_kept {
                kept[8] &= !VP8X_ICC;
            }
        }
        _ => return Err("its first chunk is not VP8, VP8L or VP8X"),
    }
    let mut out = b"RIFF".to_vec();
    out.extend(((kept.len() + 4) as u32).to_le_bytes());
    out.extend(b"WEBP");
    out.extend(kept);
    Ok(out)
}

// ---- GIF ----

/// How many bytes a colour table has, its flag and size in `packed`
/// (GIF89a, §18 and §20: three times 2 to the size plus one).
fn table(packed: u8) -> usize {
    if packed & 0x80 != 0 {
        3 << ((packed & 7) + 1)
    } else {
        0
    }
}

/// **Past a run of data sub-blocks** (GIF89a, §15): each a size and that
/// many bytes, ended by a size of zero.
fn blocks(b: &[u8], mut at: usize) -> Result<usize, &'static str> {
    loop {
        let n = *b.get(at).ok_or("it ends before its trailer")? as usize;
        at += 1 + n;
        if n == 0 {
            return Ok(at);
        }
    }
}

/// **A looping extension's loop count**, from its sub-block 1 (Netscape's
/// `NETSCAPE2.0`, and `ANIMEXTS1.0`, its other name): `ext` the extension's
/// sub-blocks, its identifier's first.
fn looping(ext: &[u8]) -> Option<[u8; 2]> {
    let id = ext.get(1..12).filter(|_| ext[0] == 11)?;
    if id != b"NETSCAPE2.0" && id != b"ANIMEXTS1.0" {
        return None;
    }
    let mut at = 12;
    loop {
        let n = *ext.get(at)? as usize;
        let data = ext.get(at + 1..at + 1 + n)?;
        if n == 3 && data[0] == 1 {
            return Some([data[1], data[2]]);
        }
        if n == 0 {
            return None;
        }
        at += 1 + n;
    }
}

/// **A GIF, its blocks walked to the trailer**: each image whole, and of
/// the extensions, a graphic control's four bytes and a loop count.
fn gif(b: &[u8]) -> Result<Vec<u8>, &'static str> {
    const SHORT: &str = "it ends before its trailer";
    let screen = 13 + table(*b.get(10).ok_or(SHORT)?);
    let mut out = b.get(..screen).ok_or(SHORT)?.to_vec();
    let mut at = screen;
    loop {
        match *b.get(at).ok_or(SHORT)? {
            0x3B => {
                out.push(0x3B);
                return Ok(out);
            }
            // An image: its descriptor, its colour table, its LZW minimum
            // code size, and its data.
            0x2C => {
                let local = table(*b.get(at + 9).ok_or(SHORT)?);
                let end = blocks(b, at + 10 + local + 1)?;
                let start = out.len();
                out.extend(b.get(at..end).ok_or(SHORT)?);
                out[start + 9] &= 0xe7;
                at = end;
            }
            0x21 => {
                let label = *b.get(at + 1).ok_or(SHORT)?;
                let end = blocks(b, at + 2)?;
                let ext = b.get(at + 2..end).ok_or(SHORT)?;
                match label {
                    // Its disposal, delay and transparency, for the image
                    // that follows: four bytes.
                    0xF9 => {
                        if ext[0] != 4 {
                            return Err("a graphic control extension is not four bytes");
                        }
                        out.extend([0x21, 0xF9]);
                        out.extend([4, ext[1] & 0x1f, ext[2], ext[3], ext[4]]);
                        out.push(0);
                    }
                    0xFF => {
                        if let Some(count) = looping(ext) {
                            out.extend([0x21, 0xFF]);
                            out.extend(&ext[..12]);
                            out.extend([3, 1, count[0], count[1], 0]);
                        }
                    }
                    // Comments (FE), plain text (01) and labels it does not know.
                    _ => {}
                }
                at = end;
            }
            _ => return Err("a block is not an image, an extension or the trailer"),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fixture a real encoder wrote (`scripts/uploads_fixtures.py`).
    pub(crate) fn fixture(name: &str) -> Vec<u8> {
        let at = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../e2e/uploads");
        std::fs::read(at.join(name)).expect("a fixture")
    }

    // ---- What names a place, a device, a person or a time: known values,
    // each searched for in what is kept. ----

    /// 48° 51′ 29.61″: `GPSLatitude`'s three RATIONALs, big-endian.
    const LATITUDE: [u32; 6] = [48, 1, 51, 1, 2961, 100];
    pub(crate) const CAMERA: &[u8] = b"PlerisPhone 7 Pro";
    pub(crate) const TAKEN: &[u8] = b"2026:10:25 09:41:00";
    pub(crate) const NAME: &[u8] = b"Jane Roe";
    pub(crate) const COMMENT: &[u8] = b"Taken at 12 Rue Example";
    /// XMP's `exif:GPSLatitude`, the same place written as text.
    const XMP_LATITUDE: &[u8] = b"48,51.4935N";
    /// tIME's 2026-10-25 09:41:00.
    const TIME: [u8; 7] = [0x07, 0xEA, 10, 25, 9, 41, 0];
    /// An ICC profile's creation time, 2026-10-25 09:41:00 (a
    /// dateTimeNumber: six big-endian 16-bit numbers).
    pub(crate) const CREATED: [u8; 12] = [0x07, 0xEA, 0, 10, 0, 25, 0, 9, 0, 41, 0, 0];
    /// An ICC profile's ID, which is an MD5 of the profile as it was.
    pub(crate) const PROFILE_ID: &[u8; 16] = b"Pleris-profileID";
    /// A display's serial number.
    pub(crate) const SERIAL: &[u8] = b"SN 4815162342";

    pub(crate) fn latitude() -> Vec<u8> {
        LATITUDE.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// Which known values `b` holds, a text's as ASCII or as UTF-16BE (as
    /// an ICC profile's `mluc` holds it).
    pub(crate) fn identified(b: &[u8]) -> Vec<&'static str> {
        let text = |s: &[u8]| vec![s.to_vec(), s.iter().flat_map(|&c| [0, c]).collect()];
        let known: [(&str, Vec<Vec<u8>>); 10] = [
            ("a GPS latitude", vec![latitude()]),
            ("an XMP latitude", text(XMP_LATITUDE)),
            ("a camera", text(CAMERA)),
            ("when it was taken", text(TAKEN)),
            ("a person's name", text(NAME)),
            ("a comment", text(COMMENT)),
            ("a PNG's tIME", vec![TIME.to_vec()]),
            ("an ICC profile's creation time", vec![CREATED.to_vec()]),
            ("an ICC profile's ID", vec![PROFILE_ID.to_vec()]),
            ("a serial number", text(SERIAL)),
        ];
        known
            .iter()
            .filter(|(_, forms)| forms.iter().any(|v| b.windows(v.len()).any(|w| w == v)))
            .map(|(what, _)| *what)
            .collect()
    }

    /// **The ICC profiles an image holds**: a JPEG's APP2 chunks before its
    /// first scan, joined in their numbers' order; each of a PNG's iCCP
    /// chunks, inflated; each of a WebP's ICCP chunks.
    pub(crate) fn profiles_in(b: &[u8]) -> Vec<Vec<u8>> {
        match sniff::sniff(b) {
            Some(Kind::Jpeg) => {
                let mut parts = Vec::new();
                let mut at = 2;
                while b[at + 1] != 0xDA {
                    let length = be16(b, at + 2).unwrap() as usize;
                    let data = &b[at + 4..at + 2 + length];
                    if b[at + 1] == 0xE2 && data.starts_with(ICC) {
                        parts.push((data[12], &data[14..]));
                    }
                    at += 2 + length;
                }
                parts.sort();
                let joined: Vec<u8> = parts.iter().flat_map(|(_, part)| part.to_vec()).collect();
                [joined].into_iter().filter(|p| !p.is_empty()).collect()
            }
            Some(Kind::Png) => {
                let mut found = Vec::new();
                let mut at = 8;
                while &b[at + 4..at + 8] != b"IEND" {
                    let length = be32(b, at).unwrap() as usize;
                    let data = &b[at + 8..at + 8 + length];
                    if &b[at + 4..at + 8] == b"iCCP" {
                        let nul = data.iter().position(|&c| c == 0).unwrap();
                        let inflated =
                            miniz_oxide::inflate::decompress_to_vec_zlib(&data[nul + 2..]);
                        found.push(inflated.expect("a zlib stream"));
                    }
                    at += 12 + length;
                }
                found
            }
            Some(Kind::Webp) => Chunks(&b[12..])
                .map(|c| c.expect("a chunk"))
                .filter(|(fourcc, _)| fourcc == b"ICCP")
                .map(|(_, data)| data.to_vec())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `profile` as a JPEG's APP2 segments of at most `part` of its bytes
    /// each, numbered from 1.
    fn app2s(profile: &[u8], part: usize) -> Vec<(u8, Vec<u8>)> {
        let parts: Vec<&[u8]> = profile.chunks(part).collect();
        let count = parts.len() as u8;
        let numbered = parts.iter().zip(1..);
        numbered
            .map(|(part, n)| (0xE2, [ICC, &[n, count], part].concat()))
            .collect()
    }

    type Entry = (u16, u16, u32, Vec<u8>);

    /// A TIFF IFD at offset `at` (big-endian): its entries, no next IFD,
    /// then each value over four bytes, at the offset its entry names.
    fn ifd(at: usize, entries: &[Entry]) -> Vec<u8> {
        let mut head = (entries.len() as u16).to_be_bytes().to_vec();
        let mut values = Vec::new();
        let past = at + 2 + 12 * entries.len() + 4;
        for (tag, kind, count, value) in entries {
            head.extend(tag.to_be_bytes());
            head.extend(kind.to_be_bytes());
            head.extend(count.to_be_bytes());
            if value.len() <= 4 {
                let mut inline = value.clone();
                inline.resize(4, 0);
                head.extend(inline);
            } else {
                head.extend(((past + values.len()) as u32).to_be_bytes());
                values.extend(value);
                if values.len() % 2 == 1 {
                    values.push(0);
                }
            }
        }
        head.extend(0u32.to_be_bytes());
        head.extend(values);
        head
    }

    /// **The Exif a phone writes** (TIFF 6.0, big-endian): IFD0's `Make`,
    /// `Orientation`, `DateTime` and `GPSInfo`, and its GPS IFD's
    /// `GPSLatitudeRef` and `GPSLatitude`.
    pub(crate) fn tiff(orientation: u16) -> Vec<u8> {
        let ascii = |s: &[u8]| [s, b"\0"].concat();
        let ifd0 = |gps: u32| {
            ifd(
                8,
                &[
                    (0x010F, 2, CAMERA.len() as u32 + 1, ascii(CAMERA)),
                    (0x0112, 3, 1, orientation.to_be_bytes().to_vec()),
                    (0x0132, 2, TAKEN.len() as u32 + 1, ascii(TAKEN)),
                    (0x8825, 4, 1, gps.to_be_bytes().to_vec()),
                ],
            )
        };
        let gps = 8 + ifd0(0).len();
        let mut t = b"MM\0\x2A".to_vec();
        t.extend(8u32.to_be_bytes());
        t.extend(ifd0(gps as u32));
        t.extend(ifd(
            gps,
            &[(0x0001, 2, 2, b"N\0".to_vec()), (0x0002, 5, 3, latitude())],
        ));
        t
    }

    /// **An XMP packet** that names its creator and where it was taken.
    pub(crate) fn xmp() -> Vec<u8> {
        [
            &b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
               xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description \
               rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
               xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" exif:GPSLatitude=\""[..],
            XMP_LATITUDE,
            b"\"><dc:creator><rdf:Seq><rdf:li>",
            NAME,
            b"</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>",
        ]
        .concat()
    }

    /// **IPTC's By-line** (IIM record 2, dataset 80), in Photoshop's APP13.
    fn iptc() -> Vec<u8> {
        let mut iim = vec![0x1C, 0x02, 0x50];
        iim.extend((NAME.len() as u16).to_be_bytes());
        iim.extend(NAME);
        let mut d = b"Photoshop 3.0\0".to_vec();
        d.extend(b"8BIM");
        d.extend(0x0404u16.to_be_bytes());
        d.extend([0, 0]);
        d.extend((iim.len() as u32).to_be_bytes());
        d.extend(iim);
        d
    }

    // ---- JPEG ----

    /// SOI, `segments`, `body`, then `after`.
    fn jpeg_of(segments: &[(u8, Vec<u8>)], body: &[u8], after: &[u8]) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        for (m, data) in segments {
            segment(&mut b, *m, data);
        }
        b.extend(body);
        b.extend(after);
        b
    }

    /// `eight-by-five.jpeg` past its JFIF APP0: its tables, frame, scan and
    /// EOI.
    fn jpeg_body() -> Vec<u8> {
        let base = fixture("eight-by-five.jpeg");
        assert_eq!(
            base[2..6],
            [0xFF, 0xE0, 0x00, 0x10],
            "JFIF's APP0, 14 bytes"
        );
        base[20..].to_vec()
    }

    /// JFIF 1.02's APP0 at 144 dots per inch (a Retina screen's), with a 2
    /// by 1 thumbnail.
    fn jfif_with_thumbnail() -> Vec<u8> {
        let mut d = b"JFIF\0\x01\x02\x01\x00\x90\x00\x90\x02\x01".to_vec();
        d.extend([0xB0, 0x0B, 0x1E, 0x55, 0x0F, 0xF1]);
        d
    }

    /// JFIF 1.02's APP0 as it is kept: its version, no units, and a density
    /// of 1 by 1, the pixels' aspect alone.
    const JFIF_KEPT: &[u8] = b"JFIF\0\x01\x02\x00\x00\x01\x00\x01\x00\x00";

    /// **A JPEG with restart markers** (T.81, B.2.4.4's DRI and B.1.1.5's
    /// RSTm in a scan), as a phone's or a camera's hardware encoder writes
    /// them, built here: 16 by 8, three components each sampled 1 by 1, a
    /// restart after each MCU. Each block is its DC alone (its quantizer 8,
    /// Huffman codes `0` for category 0 and `10` for 7, and `0` for EOB):
    /// the left MCU's Y −64, so grey 64; the right's +64 after the restart
    /// has reset the prediction, so grey 192.
    pub(crate) fn restarted_jpeg() -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        segment(&mut b, 0xDB, &[&[0x00, 8][..], &[1; 63]].concat());
        segment(
            &mut b,
            0xC0,
            &[8, 0, 8, 0, 16, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0],
        );
        let dc = [&[0x00, 1, 1][..], &[0; 14], &[0, 7]].concat();
        let ac = [&[0x10, 1][..], &[0; 15], &[0x00]].concat();
        segment(&mut b, 0xC4, &[dc, ac].concat());
        segment(&mut b, 0xDD, &[0, 1]);
        segment(&mut b, 0xDA, &[3, 1, 0x00, 2, 0x00, 3, 0x00, 0, 63, 0]);
        // Y: `10`, then −64's seven bits `0111111`, then EOB; Cb and Cr: DC
        // 0 and EOB; padded with 1s.
        b.extend([0x9F, 0x83]);
        b.extend([0xFF, 0xD0]);
        // Y: `10`, then +64's `1000000`, then EOB; Cb and Cr.
        b.extend([0xA0, 0x03]);
        b.extend([0xFF, 0xD9]);
        b
    }

    /// Adobe's APP14 (version 100, no flags, transform 1), and two bytes
    /// past its twelve.
    fn adobe() -> Vec<u8> {
        [&b"Adobe\x00\x64\x00\x00\x00\x00\x01"[..], b"JR"].concat()
    }

    /// **A phone's photo**: JFIF at 144 dots per inch with a thumbnail, and
    /// JFXX's; Exif turned a quarter, with its camera, time and GPS
    /// position; an ICC profile, a display's (version 2), which names a
    /// person, a device, its serial number and a time, in two APP2 chunks,
    /// the second first; XMP; IPTC; MPF; C2PA's JUMBF; Adobe's APP14;
    /// APP15; a comment; then the eight-by-five image, and after its EOI a
    /// second image with its own Exif, as a phone appends a depth map.
    pub(crate) fn laden_jpeg() -> Vec<u8> {
        let second = jpeg_of(&[(0xE1, [EXIF, &tiff(1)].concat())], &[0xFF, 0xD9], &[]);
        let icc = app2s(&icc::tests::planted(2), 600);
        jpeg_of(
            &[
                (0xE0, jfif_with_thumbnail()),
                (0xE0, b"JFXX\0\x13\x01\x01\x0F\xF1\xCE".to_vec()),
                (0xE1, [EXIF, &tiff(6)].concat()),
                icc[1].clone(),
                (
                    0xE1,
                    [&b"http://ns.adobe.com/xap/1.0/\0"[..], &xmp()].concat(),
                ),
                icc[0].clone(),
                (0xED, iptc()),
                (0xE2, [&b"MPF\0MM\0\x2A\0\0\0\x08"[..], NAME].concat()),
                (0xEB, [&b"JP\0\x01\0\0\0\x01jumb"[..], NAME].concat()),
                (0xEE, adobe()),
                (0xEF, CAMERA.to_vec()),
                (0xFE, COMMENT.to_vec()),
            ],
            &jpeg_body(),
            &second,
        )
    }

    #[test]
    fn a_jpeg_keeps_its_image_colour_and_turn_and_nothing_else() {
        let laden = laden_jpeg();
        assert_eq!(identified(&laden).len(), 9, "{:?}", identified(&laden));
        let kept = strip(&laden).expect("kept");
        assert_eq!(identified(&kept), Vec::<&str>::new());
        // Exactly: JFIF without its density or thumbnail, the turn alone,
        // the ICC profile rewritten (`icc.rs`) in one chunk, Adobe's twelve
        // bytes, and the image's every byte.
        let profile = icc::scrub(&icc::tests::planted(2)).expect("a profile kept");
        let expected = jpeg_of(
            &[
                (0xE0, JFIF_KEPT.to_vec()),
                (0xE1, orientation_alone(6)),
                (0xE2, [ICC, &[1, 1], &profile].concat()),
                (0xEE, adobe()[..12].to_vec()),
            ],
            &jpeg_body(),
            &[],
        );
        assert_eq!(kept, expected);
        // Shown turned, as the original: 8 by 5 stored, 5 by 8 shown.
        let shown = sniff::measure(&kept).expect("measured");
        assert_eq!((shown.width, shown.height), (5, 8));
        assert_eq!(strip(&kept), Ok(kept.clone()), "rewritten to itself");
    }

    #[test]
    fn every_valid_jpeg_orientation_is_kept_alone() {
        let body = jpeg_body();
        for o in 0..=9 {
            let sent = jpeg_of(&[(0xE1, [EXIF, &tiff(o)].concat())], &body, &[]);
            let segments = if (2..=8).contains(&o) {
                vec![(0xE1, orientation_alone(o))]
            } else {
                vec![]
            };
            let expected = jpeg_of(&segments, &body, &[]);
            let kept = strip(&sent).expect("JPEG kept");
            assert_eq!(kept, expected, "orientation {o}");
            let shown = sniff::measure(&kept).expect("measured");
            let size = if (5..=8).contains(&o) { (5, 8) } else { (8, 5) };
            assert_eq!((shown.width, shown.height), size, "orientation {o}");
            assert_eq!(strip(&kept), Ok(kept.clone()));
        }
    }

    #[test]
    fn jpeg_fill_bytes_before_segments_and_after_scans_are_accepted() {
        let body = jpeg_body();
        let base = jpeg_of(&[(0xE1, orientation_alone(6))], &body, &[]);
        for count in [1, 2, 17] {
            let mut sent = base.clone();
            sent.splice(2..2, std::iter::repeat_n(0xff, count));
            let eoi = sent.len() - 2;
            sent.splice(eoi..eoi, std::iter::repeat_n(0xff, count));
            assert_eq!(strip(&sent), Ok(base.clone()), "{count} fill bytes");
        }
    }

    #[test]
    fn fill_bytes_before_a_jpeg_restart_stay_with_the_scan() {
        let base = restarted_jpeg();
        let restart = base.windows(2).position(|w| w == [0xff, 0xd0]).unwrap();
        for count in [2, 17] {
            let mut sent = base.clone();
            sent.splice(restart..restart, std::iter::repeat_n(0xff, count));
            assert_eq!(strip(&sent), Ok(sent.clone()), "restart fill {count}");
        }
    }

    #[test]
    fn short_jfif_and_adobe_segments_are_dropped_without_panicking() {
        let body = jpeg_body();
        for (marker, data, required) in [(0xe0, jfif_with_thumbnail(), 14), (0xee, adobe(), 12)] {
            for len in 0..required {
                let sent = jpeg_of(&[(marker, data[..len].to_vec())], &body, &[]);
                let result = std::panic::catch_unwind(|| strip(&sent));
                assert_eq!(
                    result.expect("short header must not panic"),
                    Ok(jpeg_of(&[], &body, &[])),
                    "{marker:x}, {len}"
                );
            }
            let sent = jpeg_of(&[(marker, data[..required].to_vec())], &body, &[]);
            assert!(strip(&sent).is_ok(), "complete header");
        }
    }

    #[test]
    fn a_jpegs_turn_is_its_first_readable_exif_before_any_scan() {
        let body = jpeg_body();
        let mut unreadable = [EXIF, &tiff(6)].concat();
        unreadable[EXIF.len() + 2] = 41; // not TIFF's 42
        let exif = |o| (0xE1, [EXIF, &tiff(o)].concat());
        let kept = strip(&jpeg_of(
            &[(0xE1, unreadable), exif(8), exif(6)],
            &body,
            &[],
        ));
        let turned = jpeg_of(&[(0xE1, orientation_alone(8))], &body, &[]);
        assert_eq!(kept, Ok(turned));
        // Unturned (1) and out of range (9): no turn is written.
        for o in [1, 9] {
            assert_eq!(
                strip(&jpeg_of(&[exif(o)], &body, &[])),
                Ok(jpeg_of(&[], &body, &[]))
            );
        }
        // An Exif APP1 after the first scan is not read, as a browser
        // reads none there.
        let progressive = fixture("nine-by-seven-progressive.jpeg");
        let first_scan = progressive
            .windows(2)
            .position(|w| w == [0xFF, 0xDA])
            .unwrap();
        let after = first_scan
            + progressive[first_scan..]
                .windows(2)
                .position(|w| w == [0xFF, 0xC4])
                .unwrap();
        let mut late = progressive[..after].to_vec();
        let (m, data) = exif(6);
        segment(&mut late, m, &data);
        segment(&mut late, 0xFE, COMMENT);
        late.extend(&progressive[after..]);
        assert_eq!(strip(&late), Ok(progressive), "its scans kept whole");
    }

    #[test]
    fn a_jpeg_that_does_not_parse_is_refused() {
        let laden = laden_jpeg();
        let malformed = |b: &[u8]| match strip(b) {
            Err(Unreadable::Malformed(Kind::Jpeg, why)) => why,
            other => panic!("not refused: {other:?}"),
        };
        let body = jpeg_body();
        let eoi = body.len() - 2;
        assert_eq!(
            malformed(&jpeg_of(&[], &body[..eoi], &[])),
            "it ends before its EOI"
        );
        assert_eq!(
            malformed(&laden[..100]),
            "it ends before its EOI",
            "cut in a segment"
        );
        assert_eq!(
            body[..4],
            [0xFF, 0xDB, 0x00, 0x43],
            "a DQT of 69 bytes first"
        );
        let between = jpeg_of(&[], &body[..69], &[]);
        assert_eq!(
            malformed(&between),
            "it ends before its EOI",
            "cut between two"
        );
        let second_soi = jpeg_of(&[], &[&[0xFF, 0xD8][..], &body].concat(), &[]);
        assert_eq!(malformed(&second_soi), "a second SOI");
        let stray = jpeg_of(&[(0xFE, vec![])], &[&[0x00][..], &body].concat(), &[]);
        assert_eq!(malformed(&stray), "a segment is not followed by a marker");
        let stuffed = jpeg_of(&[], &[&[0xFF, 0x00][..], &body].concat(), &[]);
        assert_eq!(malformed(&stuffed), "a stuffed zero where a marker is");
        let mut under = jpeg_of(&[(0xFE, vec![])], &body, &[]);
        under[5] = 1;
        assert_eq!(malformed(&under), "a segment's length is under 2");
    }

    /// **A JPEG's density is not kept** (the independent review, finding
    /// 2): JFIF's units and densities name the screen or the scanner that
    /// made it (a Retina screen's 144 dots per inch), as PNG's pHYs does,
    /// which is dropped; no engine sizes an image by them. What is kept says
    /// no units and 1 by 1, the pixels square.
    #[test]
    fn a_jpegs_density_is_not_kept() {
        let body = jpeg_body();
        let jfif = |units: u8, x: u16, y: u16| {
            let d = [
                &b"JFIF\0\x01\x02"[..],
                &[units],
                &x.to_be_bytes(),
                &y.to_be_bytes(),
            ];
            (0xE0, [&d.concat()[..], &[0, 0]].concat())
        };
        let kept = jpeg_of(&[(0xE0, JFIF_KEPT.to_vec())], &body, &[]);
        for (units, x, y) in [(1, 144, 144), (2, 57, 57), (1, 72, 300), (0, 2, 1)] {
            let sent = jpeg_of(&[jfif(units, x, y)], &body, &[]);
            assert_eq!(strip(&sent), Ok(kept.clone()), "units {units}, {x} by {y}");
        }
    }

    /// **A JPEG's ICC profile is read as libjpeg reads its chunks, and kept
    /// for its colour alone** (the independent review, finding 1;
    /// `jpeg_read_icc_profile` in libjpeg-turbo's `jdicc.c`, on the APP2
    /// segments before the first scan): its chunks joined in their numbers'
    /// order, wherever they are; rewritten (`icc.rs`); and split again into
    /// chunks of 65,519 bytes, as many as an APP2 segment holds. Chunks that
    /// do not make one profile, as libjpeg reads them, are not kept, nor a
    /// profile that does not parse, nor one after the first scan, which no
    /// browser reads; an APP2 too short to number its chunk is no chunk.
    #[test]
    fn a_jpegs_icc_profile_is_read_as_libjpeg_reads_its_chunks() {
        let body = jpeg_body();
        let large = [icc::tests::naming(4), icc::tests::colour(4, 40_000)].concat();
        let profile = icc::tests::profile(4, &large);
        assert!(profile.len() > 65_519, "more than one chunk's");
        let scrubbed = icc::scrub(&profile).expect("a profile kept");
        assert_eq!(identified(&scrubbed), Vec::<&str>::new());
        let app2 = |n: u8, count: u8, part: &[u8]| (0xE2, [ICC, &[n, count], part].concat());
        let (one, two) = profile.split_at(30_000);
        let (a, b, c) = (&one[..15_000], &one[15_000..], two);
        let sent = jpeg_of(
            &[app2(2, 2, two), (0xFE, COMMENT.to_vec()), app2(1, 2, one)],
            &body,
            &[],
        );
        let (first, second) = scrubbed.split_at(65_519);
        let kept = jpeg_of(&[app2(1, 2, first), app2(2, 2, second)], &body, &[]);
        assert_eq!(strip(&sent), Ok(kept.clone()));
        assert_eq!(strip(&kept), Ok(kept.clone()), "rewritten to itself");
        assert_eq!(profiles_in(&kept), vec![scrubbed]);
        let none = jpeg_of(&[], &body, &[]);
        let small = icc::tests::planted(2);
        let cases = [
            ("a chunk missing", vec![app2(1, 2, one)]),
            (
                "a chunk twice",
                vec![app2(1, 2, one), app2(1, 2, one), app2(2, 2, two)],
            ),
            (
                "their counts differ, though they hold it all",
                vec![app2(1, 2, a), app2(2, 2, b), app2(3, 3, c)],
            ),
            ("a chunk numbered 0", vec![app2(0, 2, one), app2(2, 2, two)]),
            (
                "a chunk numbered past its count",
                vec![app2(1, 1, one), app2(2, 1, two)],
            ),
            (
                "a profile that does not parse",
                vec![app2(1, 1, &small[1..])],
            ),
        ];
        for (what, segments) in cases {
            assert_eq!(
                strip(&jpeg_of(&segments, &body, &[])),
                Ok(none.clone()),
                "{what}"
            );
        }
        let alone = jpeg_of(&[app2(1, 1, &icc::scrub(&small).unwrap())], &body, &[]);
        let short = (0xE2, [ICC, &[1]].concat());
        assert_eq!(
            strip(&jpeg_of(&[app2(1, 1, &small), short], &body, &[])),
            Ok(alone)
        );
        // After the first scan: not read, so not kept.
        let progressive = fixture("nine-by-seven-progressive.jpeg");
        let first_scan = progressive
            .windows(2)
            .position(|w| w == [0xFF, 0xDA])
            .unwrap();
        let after = first_scan
            + progressive[first_scan..]
                .windows(2)
                .position(|w| w == [0xFF, 0xC4])
                .unwrap();
        let mut late = progressive[..after].to_vec();
        let (m, data) = app2(1, 1, &small);
        segment(&mut late, m, &data);
        late.extend(&progressive[after..]);
        assert_eq!(strip(&late), Ok(progressive));
    }

    /// **A JPEG with restart markers is kept whole** (the independent
    /// review, finding 3): a restart marker is within its scan, not its end,
    /// so the scan is copied past it.
    #[test]
    fn a_jpeg_with_restart_markers_is_kept_whole() {
        let restarted = restarted_jpeg();
        let restarts = restarted
            .windows(2)
            .filter(|w| w[0] == 0xFF && (0xD0..=0xD7).contains(&w[1]));
        assert_eq!(restarts.count(), 1, "a restart in its scan");
        assert_eq!(strip(&restarted), Ok(restarted.clone()));
        let shown = sniff::measure(&restarted).expect("measured");
        assert_eq!((shown.width, shown.height), (16, 8));
        // With what a camera adds, its scan is kept whole all the same.
        let laden = [
            &restarted[..2],
            &jpeg_of(
                &[(0xE1, [EXIF, &tiff(1)].concat()), (0xFE, COMMENT.to_vec())],
                &[],
                &[],
            )[2..],
            &restarted[2..],
        ]
        .concat();
        assert_eq!(identified(&laden).len(), 4, "{:?}", identified(&laden));
        assert_eq!(strip(&laden), Ok(restarted));
    }

    // ---- PNG ----

    /// An iCCP chunk: its profile's name, a NUL, compression method 0, and
    /// `profile` deflated at `level`, in zlib's stream.
    pub(crate) fn iccp_chunk(name: &[u8], profile: &[u8], level: u8) -> Vec<u8> {
        let deflated = miniz_oxide::deflate::compress_to_vec_zlib(profile, level);
        png_chunk(b"iCCP", &[name, &[0, 0], &deflated].concat())
    }

    /// `six-by-four.png`: its signature and IHDR, its IDAT, its IEND.
    fn png_parts() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let base = fixture("six-by-four.png");
        assert_eq!((&base[37..41], &base[73..77]), (&b"IDAT"[..], &b"IEND"[..]));
        (
            base[..33].to_vec(),
            base[33..69].to_vec(),
            base[69..].to_vec(),
        )
    }

    /// The colour, transparency and animation chunks a PNG keeps, before its
    /// IDAT and after it, `iccp` its iCCP chunk.
    fn png_kept(iccp: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
        let before = [
            png_chunk(b"gAMA", &[0, 0, 0xB1, 0x8F]),
            png_chunk(b"cHRM", &[7; 32]),
            iccp,
            png_chunk(b"sRGB", &[0]),
            png_chunk(b"cICP", &[1, 13, 0, 1]),
            png_chunk(b"mDCV", &[9; 24]),
            png_chunk(b"cLLI", &[0, 0, 3, 232, 0, 0, 1, 144]),
            png_chunk(b"PLTE", &[255, 0, 0, 0, 255, 0]),
            png_chunk(b"tRNS", &[0, 1, 0, 2, 0, 3]),
            png_chunk(b"acTL", &[0, 0, 0, 2, 0, 0, 0, 0]),
            png_chunk(b"fcTL", &[0; 26]),
        ];
        let after = [
            png_chunk(b"fcTL", &[1; 26]),
            png_chunk(b"fdAT", &[0, 0, 0, 2, 9, 9]),
        ];
        (before.concat(), after.concat())
    }

    /// **A PNG an editor wrote**: six-by-four.png with the chunks it keeps,
    /// its iCCP a display's profile (version 4) named for a person; and
    /// with text (tEXt, zTXt, iTXt's XMP), tIME, eXIf, pHYs, sBIT, bKGD and
    /// a private chunk among them, and bytes after its IEND.
    pub(crate) fn laden_png() -> Vec<u8> {
        let (head, idat, iend) = png_parts();
        let (before, after) = png_kept(iccp_chunk(NAME, &icc::tests::planted(4), 9));
        [
            head,
            png_chunk(b"tEXt", &[&b"Comment\0"[..], COMMENT].concat()),
            png_chunk(b"eXIf", &tiff(6)),
            before,
            png_chunk(b"zTXt", &[&b"Author\0\0"[..], NAME].concat()),
            png_chunk(b"pHYs", &[0, 0, 0x16, 0x25, 0, 0, 0x16, 0x25, 1]),
            png_chunk(b"sBIT", &[8, 8, 8]),
            png_chunk(b"sPLT", &[&b"Jane Roe\0\x08"[..], &[0; 6]].concat()),
            png_chunk(b"hIST", &[0, 1, 0, 2]),
            png_chunk(b"bKGD", &[0, 0, 0, 0, 0, 0]),
            png_chunk(b"prVt", CAMERA),
            idat,
            png_chunk(
                b"iTXt",
                &[&b"XML:com.adobe.xmp\0\0\0\0\0"[..], &xmp()].concat(),
            ),
            png_chunk(b"tIME", &TIME),
            after,
            iend,
            COMMENT.to_vec(),
        ]
        .concat()
    }

    #[test]
    fn a_png_keeps_its_image_colour_and_frames_and_nothing_else() {
        let laden = laden_png();
        assert_eq!(identified(&laden).len(), 7, "{:?}", identified(&laden));
        let planted = icc::tests::planted(4);
        assert_eq!(
            identified(&profiles_in(&laden).concat()).len(),
            6,
            "in its profile"
        );
        let kept = strip(&laden).expect("kept");
        assert_eq!(identified(&kept), Vec::<&str>::new());
        // Its iCCP named "ICC profile", its profile rewritten (`icc.rs`),
        // deflated again, and its CRC its own.
        let (head, idat, iend) = png_parts();
        let profile = icc::scrub(&planted).expect("a profile kept");
        let (before, after) = png_kept(iccp_chunk(icc::NEUTRAL, &profile, 6));
        assert_eq!(kept, [head, before, idat, after, iend].concat());
        assert_eq!(profiles_in(&kept), vec![profile]);
        assert_eq!(strip(&kept), Ok(kept.clone()), "rewritten to itself");
    }

    /// **A PNG's ICC profile is read from its first iCCP chunk, and kept
    /// for its colour alone** (the independent review, finding 1; PNG,
    /// third edition, §11.3.2.4): its name, which is free text, made "ICC
    /// profile"; its profile inflated (with this crate's `miniz_oxide`, to
    /// at most 4 MiB), rewritten (`icc.rs`) and deflated again; its CRC
    /// computed. A second iCCP is not kept (a PNG has one), nor an iCCP
    /// without a name, with a name past 79 bytes, compressed by another
    /// method, whose stream is cut or inflates past 4 MiB, or whose profile
    /// does not parse.

    #[test]
    fn the_real_srgb_profile_survives_each_supported_container() {
        let profile = include_bytes!("../../../e2e/uploads/sRGB2014.icc");
        let expected = icc::scrub(profile).expect("real profile kept");
        let (head, idat, iend) = png_parts();
        let (before, after) = png_kept(iccp_chunk(b"sRGB", profile, 6));
        let png = [head, before, idat, after, iend].concat();
        let webp = riff_of(
            &[
                riff_chunk(b"VP8X", &[0x20, 0, 0, 0, 6, 0, 0, 2, 0, 0]),
                riff_chunk(b"ICCP", profile),
                fixture("seven-by-three-lossless.webp")[12..].to_vec(),
            ],
            &[],
        );
        let jpeg = jpeg_of(&app2s(profile, 600), &jpeg_body(), &[]);
        for sent in [jpeg, png, webp] {
            let kept = strip(&sent).expect("image kept");
            assert_eq!(profiles_in(&kept), vec![expected.clone()]);
            assert_eq!(strip(&kept), Ok(kept.clone()));
        }
    }

    #[test]
    fn a_pngs_icc_profile_is_read_from_its_first_iccp() {
        let (head, idat, iend) = png_parts();
        let png = |chunks: &[Vec<u8>]| [&head[..], &chunks.concat(), &idat, &iend].concat();
        let planted = icc::tests::planted(4);
        let kept = iccp_chunk(icc::NEUTRAL, &icc::scrub(&planted).unwrap(), 6);
        let second = iccp_chunk(b"second", &icc::tests::planted(2), 1);
        assert_eq!(
            strip(&png(&[iccp_chunk(NAME, &planted, 1), second])),
            Ok(png(std::slice::from_ref(&kept)))
        );
        let crc = be32(&kept, kept.len() - 4).unwrap();
        assert_eq!(
            crc,
            sniff::crc32(&kept[4..kept.len() - 4]),
            "its CRC its own"
        );
        let iccp = |name: &[u8], method: u8, stream: &[u8]| {
            png_chunk(b"iCCP", &[name, &[0, method], stream].concat())
        };
        let stream = miniz_oxide::deflate::compress_to_vec_zlib(&planted, 1);
        let past = [&planted[..], &vec![0; icc::MAX]].concat();
        let cases = [
            ("no name", iccp(b"", 0, &stream)),
            ("a name of 80 bytes", iccp(&[b'n'; 80], 0, &stream)),
            ("another compression method", iccp(NAME, 1, &stream)),
            ("its stream cut", iccp(NAME, 0, &stream[..stream.len() - 4])),
            ("its stream past 4 MiB", iccp_chunk(NAME, &past, 1)),
            (
                "a profile that does not parse",
                iccp_chunk(NAME, &planted[1..], 1),
            ),
        ];
        for (what, chunk) in cases {
            assert_eq!(strip(&png(&[chunk])), Ok(png(&[])), "{what}");
        }
        let longest = iccp(&[b'n'; 79], 0, &stream);
        assert_eq!(
            strip(&png(&[longest])),
            Ok(png(&[kept])),
            "a name of 79 bytes"
        );
    }

    #[test]
    fn fixed_png_chunks_refuse_undeclared_payload_bytes() {
        let (head, idat, iend) = png_parts();
        for (kind, size) in [
            (b"IHDR", 13),
            (b"IEND", 0),
            (b"cHRM", 32),
            (b"gAMA", 4),
            (b"sRGB", 1),
            (b"cICP", 4),
            (b"mDCV", 24),
            (b"cLLI", 8),
            (b"acTL", 8),
            (b"fcTL", 26),
        ] {
            let data = [vec![0; size], NAME.to_vec()].concat();
            let sent = [
                head.clone(),
                png_chunk(kind, &data),
                idat.clone(),
                iend.clone(),
            ]
            .concat();
            assert!(
                matches!(strip(&sent), Err(Unreadable::Malformed(Kind::Png, _))),
                "{kind:?} cannot hold extra bytes"
            );
        }
        assert!(
            strip(&fixture("six-by-four.png")).is_ok(),
            "real encoder neighbour"
        );
    }

    #[test]
    fn an_adobe_headers_flags_cannot_name_a_device() {
        let mut data = adobe();
        data[5..7].copy_from_slice(b"ID");
        data[7..11].copy_from_slice(b"SN42");
        let sent = jpeg_of(&[(0xee, data)], &jpeg_body(), &[]);
        let mut expected = adobe()[..12].to_vec();
        expected[7..11].fill(0);
        assert_eq!(
            strip(&sent),
            Ok(jpeg_of(&[(0xee, expected)], &jpeg_body(), &[]))
        );
    }

    #[test]
    fn webp_reserved_header_bits_are_neutral() {
        let (mut header, alph, vp8) = alpha_parts();
        header[0] |= 0xc1;
        header[1..4].copy_from_slice(b"ID!");
        let sent = riff_of(
            &[riff_chunk(b"VP8X", &header), alph.clone(), vp8.clone()],
            &[],
        );
        header[0] &= 0x3e;
        header[1..4].fill(0);
        let expected = riff_of(&[riff_chunk(b"VP8X", &header), alph, vp8], &[]);
        assert_eq!(strip(&sent), Ok(expected.clone()));
        assert_eq!(strip(&expected), Ok(expected.clone()));
    }

    #[test]
    fn png_checksums_and_transparency_cannot_hold_ignored_bytes() {
        let base = fixture("six-by-four.png");
        let mut sent = base.clone();
        sent[29..33].copy_from_slice(b"SN42");
        assert_eq!(strip(&sent), Ok(base), "CRC regenerated, not copied");
        let (head, idat, iend) = png_parts();
        let extra = png_chunk(b"tRNS", &[0; 14]);
        assert!(
            strip(&[head, extra, idat, iend].concat()).is_err(),
            "RGB transparency is six bytes"
        );
    }

    #[test]
    fn webp_animation_and_alpha_headers_cannot_carry_reserved_data() {
        let (mut header, mut alph, vp8) = alpha_parts();
        alph[8] |= 0xc0;
        let sent = riff_of(
            &[riff_chunk(b"VP8X", &header), alph.clone(), vp8.clone()],
            &[],
        );
        alph[8] &= 0x3f;
        let expected = riff_of(&[riff_chunk(b"VP8X", &header), alph, vp8.clone()], &[]);
        assert_eq!(strip(&sent), Ok(expected));
        header[0] |= 2;
        let mut frame = vec![0; 16];
        frame[15] = 0xfc;
        frame.extend(&vp8);
        let sent = riff_of(
            &[
                riff_chunk(b"VP8X", &header),
                riff_chunk(b"ANIM", &[0; 6]),
                riff_chunk(b"ANMF", &frame),
            ],
            &[],
        );
        frame[15] = 0;
        let expected = riff_of(
            &[
                riff_chunk(b"VP8X", &header),
                riff_chunk(b"ANIM", &[0; 6]),
                riff_chunk(b"ANMF", &frame),
            ],
            &[],
        );
        assert_eq!(strip(&sent), Ok(expected));
        let extra = riff_of(
            &[
                riff_chunk(b"VP8X", &header),
                riff_chunk(b"ANIM", b"Jane Roe"),
            ],
            &[],
        );
        assert!(strip(&extra).is_err(), "ANIM has six bytes");
    }

    #[test]
    fn a_gifs_reserved_descriptor_and_control_bits_are_zeroed() {
        let base = local_table_gif();
        let image = base.iter().position(|&b| b == 0x2c).unwrap();
        let mut sent = base.clone();
        sent[image + 9] |= 0x18;
        assert_eq!(strip(&sent), Ok(base));
        let sent = [
            sent[..image].to_vec(),
            vec![0x21, 0xf9, 4, 0xe0, 0, 0, 0, 0],
            sent[image..].to_vec(),
        ]
        .concat();
        let kept = strip(&sent).expect("GIF kept");
        assert_eq!(kept[image + 3], 0, "graphic control reserved bits");
    }

    #[test]
    fn a_png_that_does_not_parse_is_refused() {
        let (head, idat, iend) = png_parts();
        let malformed = |b: &[u8]| match strip(b) {
            Err(Unreadable::Malformed(Kind::Png, why)) => why,
            other => panic!("not refused: {other:?}"),
        };
        let short = "it ends before its IEND chunk";
        assert_eq!(malformed(&[&head[..], &idat].concat()), short);
        assert_eq!(malformed(&[&head[..], &idat[..20]].concat()), short, "cut");
        let critical = [&head[..], &png_chunk(b"QUIT", b"x"), &idat, &iend].concat();
        assert_eq!(
            malformed(&critical),
            "it has a critical chunk this does not know"
        );
        let numbered = [&head[..], &png_chunk(b"t3Xt", b"x"), &idat, &iend].concat();
        assert_eq!(malformed(&numbered), "a chunk's type is not four letters");
    }

    // ---- WebP ----

    fn riff_chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut c = Vec::new();
        chunk(&mut c, fourcc, data);
        c
    }

    /// A WebP of `chunks`, its RIFF's size theirs, and `after` past it.
    fn riff_of(chunks: &[Vec<u8>], after: &[u8]) -> Vec<u8> {
        let body = chunks.concat();
        let mut b = b"RIFF".to_vec();
        b.extend(((body.len() + 4) as u32).to_le_bytes());
        b.extend(b"WEBP");
        b.extend(body);
        b.extend(after);
        b
    }

    /// `twelve-by-nine-alpha.webp`'s VP8X data, and its ALPH and `VP8 `
    /// chunks.
    fn alpha_parts() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let base = fixture("twelve-by-nine-alpha.webp");
        assert_eq!((&base[12..16], &base[30..34]), (&b"VP8X"[..], &b"ALPH"[..]));
        (
            base[20..30].to_vec(),
            base[30..48].to_vec(),
            base[48..].to_vec(),
        )
    }

    /// **An extended WebP an editor wrote**: VP8X flagging an ICC profile,
    /// Exif and XMP; the profile (a display's, version 4, which names a
    /// person, a device, its serial number and a time), its alpha and its
    /// image; then EXIF, `XMP ` and a private chunk; and bytes after its
    /// RIFF.
    pub(crate) fn laden_webp() -> Vec<u8> {
        let (mut vp8x, alph, vp8) = alpha_parts();
        vp8x[0] |= VP8X_ICC | VP8X_EXIF | VP8X_XMP;
        riff_of(
            &[
                riff_chunk(b"VP8X", &vp8x),
                riff_chunk(b"ICCP", &icc::tests::planted(4)),
                alph,
                vp8,
                riff_chunk(b"EXIF", &tiff(6)),
                riff_chunk(b"XMP ", &xmp()),
                riff_chunk(b"prVt", CAMERA),
            ],
            COMMENT,
        )
    }

    #[test]
    fn a_webp_keeps_its_image_colour_and_alpha_and_nothing_else() {
        let laden = laden_webp();
        assert_eq!(identified(&laden).len(), 9, "{:?}", identified(&laden));
        let kept = strip(&laden).expect("kept");
        assert_eq!(identified(&kept), Vec::<&str>::new());
        let (mut vp8x, alph, vp8) = alpha_parts();
        vp8x[0] |= VP8X_ICC; // the ICC profile's flag, kept; Exif's and XMP's cleared
        let profile = icc::scrub(&icc::tests::planted(4)).expect("a profile kept");
        let expected = riff_of(
            &[
                riff_chunk(b"VP8X", &vp8x),
                riff_chunk(b"ICCP", &profile),
                alph,
                vp8,
            ],
            &[],
        );
        assert_eq!(kept, expected);
        assert_eq!(strip(&kept), Ok(kept.clone()), "rewritten to itself");
        // VP8X's ten bytes, and nothing a writer put after them.
        let (vp8x, alph, vp8) = alpha_parts();
        let long = riff_of(
            &[
                riff_chunk(b"VP8X", &[&vp8x[..], b"JR"].concat()),
                alph.clone(),
                vp8.clone(),
            ],
            &[],
        );
        assert_eq!(
            strip(&long),
            Ok(riff_of(&[riff_chunk(b"VP8X", &vp8x), alph, vp8], &[]))
        );
    }

    /// **A WebP's ICC profile is read from its first ICCP chunk, and kept
    /// for its colour alone** (the independent review, finding 1; RFC 9649,
    /// §2.7.1.4: one ICCP, flagged in VP8X): rewritten (`icc.rs`); a second
    /// ICCP not kept; one whose profile does not parse not kept, and VP8X
    /// then flags no profile.
    #[test]
    fn a_webps_icc_profile_is_read_from_its_first_iccp() {
        let (vp8x, alph, vp8) = alpha_parts();
        let flagged = |icc: bool| {
            let mut v = vp8x.clone();
            v[0] |= if icc { VP8X_ICC } else { 0 };
            riff_chunk(b"VP8X", &v)
        };
        let planted = icc::tests::planted(2);
        let profile = icc::scrub(&planted).expect("a profile kept");
        let two = riff_of(
            &[
                flagged(true),
                riff_chunk(b"ICCP", &planted),
                riff_chunk(b"ICCP", &icc::tests::planted(4)),
                alph.clone(),
                vp8.clone(),
            ],
            &[],
        );
        let one = riff_of(
            &[
                flagged(true),
                riff_chunk(b"ICCP", &profile),
                alph.clone(),
                vp8.clone(),
            ],
            &[],
        );
        assert_eq!(strip(&two), Ok(one));
        let unread = riff_of(
            &[
                flagged(true),
                riff_chunk(b"ICCP", &planted[1..]),
                alph.clone(),
                vp8.clone(),
            ],
            &[],
        );
        assert_eq!(
            strip(&unread),
            Ok(riff_of(&[flagged(false), alph, vp8], &[]))
        );
    }

    #[test]
    fn a_simple_webp_is_its_image_alone() {
        let base = fixture("seven-by-three-lossless.webp");
        let vp8l = base[12..].to_vec();
        let laden = riff_of(&[vp8l, riff_chunk(b"EXIF", &tiff(1))], NAME);
        assert_eq!(identified(&laden).len(), 4);
        assert_eq!(strip(&laden), Ok(base));
    }

    /// An animated WebP's frame: its 16-byte header (7 by 3 at 0, 0, for
    /// 100 ms), then `data`.
    fn frame(data: &[Vec<u8>]) -> Vec<u8> {
        [
            &[0, 0, 0, 0, 0, 0, 6, 0, 0, 2, 0, 0, 100, 0, 0, 0][..],
            &data.concat(),
        ]
        .concat()
    }

    #[test]
    fn an_animated_webp_keeps_its_frames_and_nothing_else() {
        let vp8l = fixture("seven-by-three-lossless.webp")[12..].to_vec();
        let vp8x = |flags: u8| riff_chunk(b"VP8X", &[flags, 0, 0, 0, 6, 0, 0, 2, 0, 0]);
        let anim = riff_chunk(b"ANIM", &[255, 255, 255, 255, 0, 0]);
        let laden = riff_of(
            &[
                vp8x(0x02 | VP8X_EXIF),
                anim.clone(),
                riff_chunk(b"ANMF", &frame(&[vp8l.clone(), riff_chunk(b"prVt", NAME)])),
                riff_chunk(b"ANMF", &frame(std::slice::from_ref(&vp8l))),
                riff_chunk(b"EXIF", &tiff(1)),
            ],
            &[],
        );
        assert_eq!(identified(&laden).len(), 4);
        let kept = strip(&laden).expect("kept");
        let one = riff_chunk(b"ANMF", &frame(&[vp8l]));
        assert_eq!(kept, riff_of(&[vp8x(0x02), anim, one.clone(), one], &[]));
        let measured = sniff::measure(&kept).expect("measured");
        assert_eq!((measured.width, measured.height), (7, 3));
    }

    #[test]
    fn a_webp_that_does_not_parse_is_refused() {
        let malformed = |b: &[u8]| match strip(b) {
            Err(Unreadable::Malformed(Kind::Webp, why)) => why,
            other => panic!("not refused: {other:?}"),
        };
        let mut long = laden_webp();
        let exif = long.windows(4).position(|w| w == b"EXIF").unwrap();
        long[exif + 4..exif + 8].copy_from_slice(&10_000u32.to_le_bytes());
        assert_eq!(malformed(&long), "a chunk is longer than its RIFF");
        let (vp8x, _, _) = alpha_parts();
        let short = riff_of(
            &[riff_chunk(b"VP8X", &vp8x), riff_chunk(b"ANMF", &[0; 15])],
            &[],
        );
        assert_eq!(
            malformed(&short),
            "an ANMF chunk is shorter than its frame header"
        );
    }

    /// **A WebP of five megabytes of empty chunks costs a walk, and no list
    /// of them** (the independent review, finding 4): an extended file, its
    /// image, then 624,990 empty chunks a writer made up, each eight bytes,
    /// is kept as its image alone. Its peak resident memory over the
    /// input's own stays under a quarter of the input (0 kB here), where a
    /// list of its chunks, 32 bytes each, adds four times the input (19 MB
    /// here, with the list put back); and it takes under two seconds (some
    /// 45 ms here, unoptimized). Measured in a child process of this test
    /// binary that runs this test alone, so no other test's memory is
    /// counted, by Linux's `VmHWM` (the peak resident set) read before and
    /// after, the walk run once on a small file first so that the pages of
    /// code it faults in (a megabyte or two) are not counted.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_webp_of_many_empty_chunks_is_walked_without_a_list() {
        const CHILD: &str = "PW_STRIP_WALK_CHILD";
        let vp8l = fixture("seven-by-three-lossless.webp")[12..].to_vec();
        let vp8x = riff_chunk(b"VP8X", &[0, 0, 0, 0, 6, 0, 0, 2, 0, 0]);
        if std::env::var_os(CHILD).is_some() {
            let peak = || -> u64 {
                let status = std::fs::read_to_string("/proc/self/status").expect("its status");
                let line = status.lines().find_map(|l| l.strip_prefix("VmHWM:"));
                let kb = line.expect("VmHWM").trim().trim_end_matches("kB").trim();
                kb.parse().expect("kilobytes")
            };
            // The walk's code and the measure's own read once first, so
            // that the pages of code they fault in are not counted.
            let few = riff_of(&[vp8x.clone(), vp8l.clone(), riff_chunk(b"prVt", &[])], &[]);
            assert!(strip(&few).is_ok() && peak() > 0);
            let head = 12 + vp8x.len() + vp8l.len();
            let chunks = (5_000_000 - head) / 8;
            let mut laden = Vec::with_capacity(head + 8 * chunks);
            laden.extend(b"RIFF");
            laden.extend(((head - 8 + 8 * chunks) as u32).to_le_bytes());
            laden.extend(b"WEBP");
            laden.extend(&vp8x);
            laden.extend(&vp8l);
            for _ in 0..chunks {
                laden.extend(b"prVt\0\0\0\0");
            }
            let before = peak();
            let started = std::time::Instant::now();
            let kept = strip(&laden);
            let took = started.elapsed();
            let over = peak() - before;
            assert_eq!(kept, Ok(riff_of(&[vp8x, vp8l], &[])));
            println!(
                "WALKED {} {chunks} {over} {}",
                laden.len(),
                took.as_micros()
            );
            return;
        }
        let name = "uploads::strip::tests::a_webp_of_many_empty_chunks_is_walked_without_a_list";
        let child = std::process::Command::new(std::env::current_exe().expect("this test binary"))
            .args(["--exact", name, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .expect("the child runs");
        let out = String::from_utf8_lossy(&child.stdout);
        assert!(
            child.status.success(),
            "{out}\n{}",
            String::from_utf8_lossy(&child.stderr)
        );
        // After libtest's `test <name> ... `, on its line.
        let walked: Vec<u64> = out
            .lines()
            .find_map(|l| l.split_once("WALKED ").map(|(_, measure)| measure))
            .expect("the child's measure")
            .split(' ')
            .map(|n| n.parse().expect("a count"))
            .collect();
        let [bytes, chunks, over_kb, micros] = walked[..] else {
            panic!("{out}")
        };
        assert!(bytes > 4_999_000 && chunks == 624_990, "{out}");
        assert!(
            over_kb * 1024 < bytes / 4,
            "{over_kb} kB over the input's {bytes} bytes"
        );
        assert!(micros < 2_000_000, "{micros} µs");
    }

    // ---- GIF ----

    /// `data` as GIF data sub-blocks and their terminator.
    fn sub_blocks(data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        for piece in data.chunks(255) {
            b.push(piece.len() as u8);
            b.extend(piece);
        }
        b.push(0);
        b
    }

    /// `five-by-nine.gif`'s header and logical screen with its colour table
    /// (as GIF89a), and its image.
    fn gif_parts() -> (Vec<u8>, Vec<u8>) {
        let base = fixture("five-by-nine.gif");
        assert_eq!(
            (base[10] & 0x87, base[205], base.last()),
            (0x85, 0x2C, Some(&0x3B))
        );
        let head = [&b"GIF89a"[..], &base[6..205]].concat();
        (head, base[205..base.len() - 1].to_vec())
    }

    const LOOP: [u8; 19] = *b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00";
    const CONTROL: [u8; 8] = [0x21, 0xF9, 4, 0x04, 0x0A, 0x00, 0x00, 0x00];

    /// **A GIF an editor wrote**: Netscape's loop with a second sub-block, a
    /// comment, XMP's application extension, a graphic control with a
    /// sub-block past its four bytes, plain text, the image, an extension
    /// of a label none defines, a second comment, the trailer, and bytes
    /// after it.
    pub(crate) fn laden_gif() -> Vec<u8> {
        let (head, image) = gif_parts();
        let ext = |label: u8, data: &[u8]| [&[0x21, label][..], data].concat();
        [
            head,
            ext(
                0xFF,
                &[
                    &[11][..],
                    b"NETSCAPE2.0",
                    &[3, 1, 0, 0],
                    &sub_blocks(NAME)[..],
                ]
                .concat(),
            ),
            ext(0xFE, &sub_blocks(COMMENT)),
            ext(
                0xFF,
                &[&[11][..], b"XMP DataXMP", &sub_blocks(&xmp())[..]].concat(),
            ),
            ext(0xF9, &[&CONTROL[2..7], &sub_blocks(CAMERA)[..]].concat()),
            ext(
                0x01,
                &[&[12][..], &[0; 12], &sub_blocks(TAKEN)[..]].concat(),
            ),
            image,
            ext(0x7E, &sub_blocks(CAMERA)),
            ext(0xFE, &sub_blocks(TAKEN)),
            vec![0x3B],
            COMMENT.to_vec(),
        ]
        .concat()
    }

    #[test]
    fn a_gif_keeps_its_image_timing_and_loop_and_nothing_else() {
        let laden = laden_gif();
        assert_eq!(identified(&laden).len(), 5, "{:?}", identified(&laden));
        let kept = strip(&laden).expect("kept");
        assert_eq!(identified(&kept), Vec::<&str>::new());
        let (head, image) = gif_parts();
        assert_eq!(kept, [&head[..], &LOOP, &CONTROL, &image, &[0x3B]].concat());
        assert_eq!(strip(&kept), Ok(kept.clone()), "rewritten to itself");
        // ANIMEXTS1.0 is the same extension; one with no loop says nothing.
        let other = [
            &head[..],
            &LOOP[..3],
            b"ANIMEXTS1.0",
            &LOOP[14..],
            &image,
            &[0x3B],
        ]
        .concat();
        assert_eq!(strip(&other), Ok(other.clone()));
        let no_loop = [&head[..], &LOOP[..14], &[3, 2, 0, 0, 0], &image, &[0x3B]].concat();
        assert_eq!(strip(&no_loop), Ok([&head[..], &image, &[0x3B]].concat()));
    }

    #[test]
    fn a_gif_that_does_not_parse_is_refused() {
        let (head, image) = gif_parts();
        let malformed = |b: &[u8]| match strip(b) {
            Err(Unreadable::Malformed(Kind::Gif, why)) => why,
            other => panic!("not refused: {other:?}"),
        };
        let short = "it ends before its trailer";
        assert_eq!(malformed(&[&head[..], &image].concat()), short);
        assert_eq!(
            malformed(&[&head[..], &image[..image.len() - 3]].concat()),
            short,
            "cut"
        );
        let stray = [&head[..], &[0x00], &image, &[0x3B]].concat();
        assert_eq!(
            malformed(&stray),
            "a block is not an image, an extension or the trailer"
        );
        let control = [&head[..], &[0x21, 0xF9, 3, 0, 0, 0, 0], &image, &[0x3B]].concat();
        assert_eq!(
            malformed(&control),
            "a graphic control extension is not four bytes"
        );
    }

    /// `pixels`, each an index of a colour table whose LZW minimum code
    /// size is 2, as GIF's LZW codes (GIF89a, Appendix F): a clear code (4)
    /// before every two pixels, so no code is ever wider than three bits,
    /// then the end of information (5), packed from the least significant
    /// bit.
    fn lzw(pixels: &[u8]) -> Vec<u8> {
        let mut codes = Vec::new();
        for pair in pixels.chunks(2) {
            codes.push(4);
            codes.extend(pair.iter().map(|&p| p as u32));
        }
        codes.push(5);
        let (mut bits, mut held, mut packed) = (0u32, 0, Vec::new());
        for code in codes {
            bits |= code << held;
            held += 3;
            while held >= 8 {
                packed.push(bits as u8);
                bits >>= 8;
                held -= 8;
            }
        }
        if held > 0 {
            packed.push(bits as u8);
        }
        packed
    }

    /// **A GIF whose image has its own colour table** (GIF89a, §20's local
    /// colour table), as an animation's per-frame palettes are, built here:
    /// a logical screen of 4 by 2 whose global table is black and white,
    /// and an image whose own table is red, blue, green and white, its left
    /// half red and its right half blue.
    pub(crate) fn local_table_gif() -> Vec<u8> {
        let mut b = b"GIF89a".to_vec();
        b.extend([4, 0, 2, 0, 0x80, 0, 0]);
        b.extend([0, 0, 0, 255, 255, 255]);
        b.extend([0x2C, 0, 0, 0, 0, 4, 0, 2, 0, 0x81]);
        b.extend([255, 0, 0, 0, 0, 255, 0, 255, 0, 255, 255, 255]);
        b.push(2);
        b.extend(sub_blocks(&lzw(&[0, 0, 1, 1, 0, 0, 1, 1])));
        b.push(0x3B);
        b
    }

    /// **A GIF whose images have their own colour tables is kept whole**
    /// (the independent review, finding 3): an image's local colour table
    /// is part of the image, so the walk goes on past it.
    #[test]
    fn a_gif_whose_images_have_their_own_colour_tables_is_kept() {
        let gif = local_table_gif();
        assert_eq!(gif[28], 0x81, "a local colour table of four");
        assert_eq!(strip(&gif), Ok(gif.clone()));
        let shown = sniff::measure(&gif).expect("measured");
        assert_eq!((shown.width, shown.height), (4, 2));
        // Two frames, each with its own palette, a comment between them.
        let (head, image) = (&gif[..19], &gif[19..gif.len() - 1]);
        let comment = [&[0x21, 0xFE][..], &sub_blocks(COMMENT)].concat();
        let laden = [head, &CONTROL, image, &comment, &CONTROL, image, &[0x3B]].concat();
        assert_eq!(identified(&laden), vec!["a comment"]);
        let kept = [head, &CONTROL, image, &CONTROL, image, &[0x3B]].concat();
        assert_eq!(strip(&laden), Ok(kept));
    }

    // ---- Every format ----

    /// **Each real encoder's file has nothing to drop**, so it is kept as
    /// it was, and what is not an image is no kind's.
    #[test]
    fn real_encoders_files_are_kept_as_they_are() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../e2e/uploads");
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("fixtures.json")).unwrap())
                .unwrap();
        assert_eq!(rows.len(), 10);
        for row in rows {
            let file = row["file"].as_str().unwrap();
            let bytes = fixture(file);
            match row["kind"].as_str() {
                Some(_) => assert_eq!(strip(&bytes).as_ref(), Ok(&bytes), "{file}"),
                None => assert_eq!(strip(&bytes), Err(Unreadable::Unrecognized), "{file}"),
            }
        }
    }

    /// **No input makes the rewriter fail but by refusing**, and what it
    /// keeps it keeps again unchanged: each laden file cut at every length,
    /// and with each of its bytes changed.
    #[test]
    fn any_bytes_are_refused_or_kept_and_kept_again_unchanged() {
        for laden in [laden_jpeg(), laden_png(), laden_webp(), laden_gif()] {
            let mut tried = 0;
            let mut check = |b: &[u8]| {
                if let Ok(kept) = strip(b) {
                    assert_eq!(strip(&kept).as_ref(), Ok(&kept));
                    tried += 1;
                }
            };
            for n in 0..=laden.len() {
                check(&laden[..n]);
            }
            for i in 0..laden.len() {
                let mut changed = laden.clone();
                changed[i] ^= 0xA5;
                check(&changed);
            }
            assert!(tried > laden.len() / 2, "most changed files are still kept");
        }
    }
}
