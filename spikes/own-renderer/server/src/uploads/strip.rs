//! **An image kept without what names a place, a device, a person or a
//! time** (track `upload-metadata`, ADR-XXXX, amending ADR-0260).
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
            // Its version, units and densities, and no thumbnail: a thumbnail
            // is the picture as it was before an edit cropped it.
            0xE0 if data.starts_with(JFIF) && data.len() >= 14 => {
                if jfif.is_none() {
                    jfif = Some([&data[..12], &[0, 0]].concat());
                }
            }
            // The turn a browser shows it at, as it reads it: the first Exif
            // structure whose `Orientation` it can read, before any scan.
            0xE1 if !scanned && turned.is_none() && data.starts_with(EXIF) => {
                turned = sniff::orientation(&data[EXIF.len()..]);
            }
            0xE2 if data.starts_with(ICC) => segment(&mut kept, m, data),
            0xEE if data.starts_with(ADOBE) && data.len() >= 12 => {
                segment(&mut kept, m, &data[..12]);
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

/// **The chunks a PNG keeps** (third edition, §4.8.2 and §11.3): the
/// critical four; transparency; colour space information a browser renders
/// by (sBIT, which none does, is not among them); and animation's.
const PNG_KEPT: [&[u8; 4]; 15] = [
    b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"cHRM", b"gAMA", b"iCCP", b"sRGB", b"cICP",
    b"mDCV", b"cLLI", b"acTL", b"fcTL", b"fdAT",
];

/// **A PNG, its chunks walked to IEND**: those it keeps copied whole, with
/// their CRCs, in order.
fn png(b: &[u8]) -> Result<Vec<u8>, &'static str> {
    const SHORT: &str = "it ends before its IEND chunk";
    let mut out = b.get(..8).ok_or(SHORT)?.to_vec();
    let mut at = 8;
    loop {
        let length = be32(b, at).ok_or(SHORT)? as usize;
        let kind = b.get(at + 4..at + 8).ok_or(SHORT)?;
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err("a chunk's type is not four letters");
        }
        let end = (at + 12).checked_add(length).ok_or(SHORT)?;
        let chunk = b.get(at..end).ok_or(SHORT)?;
        if PNG_KEPT.iter().any(|k| &k[..] == kind) {
            out.extend(chunk);
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
/// significant): Exif metadata (E) and XMP metadata (X).
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

/// **A RIFF list's chunks**. A last chunk of an odd size may lack its
/// padding.
fn chunks(mut b: &[u8]) -> Result<Vec<Chunk<'_>>, &'static str> {
    const LONG: &str = "a chunk is longer than its RIFF";
    let mut found = Vec::new();
    while !b.is_empty() {
        let fourcc = b.get(..4).ok_or(LONG)?;
        let size = le32(b, 4).ok_or(LONG)? as usize;
        let data = b.get(8..8 + size).ok_or(LONG)?;
        found.push((fourcc, data));
        b = b.get(8 + size + size % 2..).unwrap_or(&[]);
    }
    Ok(found)
}

/// **A WebP, its chunks walked to its RIFF's size**: a simple file's image,
/// or an extended file's image, colour and animation.
fn webp(b: &[u8]) -> Result<Vec<u8>, &'static str> {
    let size = le32(b, 4).ok_or("it ends before its RIFF header does")? as usize;
    let body = b
        .get(12..size.saturating_add(8))
        .ok_or("its RIFF size is not the bytes it has")?;
    let found = chunks(body)?;
    let mut kept = Vec::new();
    match found.first() {
        Some((first @ (b"VP8 " | b"VP8L"), data)) => chunk(&mut kept, first, data),
        Some((b"VP8X", _)) => {
            for (n, (fourcc, data)) in found.iter().enumerate() {
                match *fourcc {
                    b"VP8X" if n == 0 => {
                        let mut header = data.get(..10).ok_or("its VP8X chunk is short")?.to_vec();
                        header[0] &= !(VP8X_EXIF | VP8X_XMP);
                        chunk(&mut kept, fourcc, &header);
                    }
                    b"ICCP" | b"ANIM" | b"ALPH" | b"VP8 " | b"VP8L" => {
                        chunk(&mut kept, fourcc, data);
                    }
                    b"ANMF" => {
                        let mut frame = data
                            .get(..16)
                            .ok_or("an ANMF chunk is shorter than its frame header")?
                            .to_vec();
                        for (fourcc, data) in chunks(&data[16..])? {
                            if matches!(fourcc, b"ALPH" | b"VP8 " | b"VP8L") {
                                chunk(&mut frame, fourcc, data);
                            }
                        }
                        chunk(&mut kept, b"ANMF", &frame);
                    }
                    // EXIF, `XMP `, and chunks a writer made up.
                    _ => {}
                }
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
                out.extend(b.get(at..end).ok_or(SHORT)?);
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
                        out.extend(&ext[..5]);
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

    pub(crate) fn latitude() -> Vec<u8> {
        LATITUDE.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// Which known values `b` holds.
    pub(crate) fn identified(b: &[u8]) -> Vec<&'static str> {
        let known: [(&str, &[u8]); 7] = [
            ("a GPS latitude", &latitude()),
            ("an XMP latitude", XMP_LATITUDE),
            ("a camera", CAMERA),
            ("when it was taken", TAKEN),
            ("a person's name", NAME),
            ("a comment", COMMENT),
            ("a PNG's tIME", &TIME),
        ];
        known
            .iter()
            .filter(|(_, v)| b.windows(v.len()).any(|w| w == *v))
            .map(|(what, _)| *what)
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

    /// JFIF 1.02's APP0 with a 2 by 1 thumbnail.
    fn jfif_with_thumbnail() -> Vec<u8> {
        let mut d = b"JFIF\0\x01\x02\x01\x00\x48\x00\x48\x02\x01".to_vec();
        d.extend([0xB0, 0x0B, 0x1E, 0x55, 0x0F, 0xF1]);
        d
    }

    /// Adobe's APP14 (version 100, no flags, transform 1), and two bytes
    /// past its twelve.
    fn adobe() -> Vec<u8> {
        [&b"Adobe\x00\x64\x00\x00\x00\x00\x01"[..], b"JR"].concat()
    }

    /// **A phone's photo**: JFIF with a thumbnail, and JFXX's; Exif turned a
    /// quarter, with its camera, time and GPS position; an ICC profile; XMP;
    /// IPTC; MPF; C2PA's JUMBF; Adobe's APP14; APP15; a comment; then the
    /// eight-by-five image, and after its EOI a second image with its own
    /// Exif, as a phone appends a depth map.
    pub(crate) fn laden_jpeg() -> Vec<u8> {
        let second = jpeg_of(&[(0xE1, [EXIF, &tiff(1)].concat())], &[0xFF, 0xD9], &[]);
        jpeg_of(
            &[
                (0xE0, jfif_with_thumbnail()),
                (0xE0, b"JFXX\0\x13\x01\x01\x0F\xF1\xCE".to_vec()),
                (0xE1, [EXIF, &tiff(6)].concat()),
                (0xE2, b"ICC_PROFILE\0\x01\x01a profile".to_vec()),
                (
                    0xE1,
                    [&b"http://ns.adobe.com/xap/1.0/\0"[..], &xmp()].concat(),
                ),
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
        assert_eq!(identified(&laden).len(), 6, "{:?}", identified(&laden));
        let kept = strip(&laden).expect("kept");
        assert_eq!(identified(&kept), Vec::<&str>::new());
        // Exactly: JFIF without its thumbnail, the turn alone, the ICC
        // profile, Adobe's twelve bytes, and the image's every byte.
        let expected = jpeg_of(
            &[
                (0xE0, b"JFIF\0\x01\x02\x01\x00\x48\x00\x48\x00\x00".to_vec()),
                (0xE1, orientation_alone(6)),
                (0xE2, b"ICC_PROFILE\0\x01\x01a profile".to_vec()),
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

    // ---- PNG ----

    /// A PNG chunk, its CRC computed.
    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut c = (data.len() as u32).to_be_bytes().to_vec();
        c.extend(kind);
        c.extend(data);
        c.extend(sniff::crc32(&c[4..]).to_be_bytes());
        c
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
    /// IDAT and after it.
    fn png_kept() -> (Vec<u8>, Vec<u8>) {
        let before = [
            png_chunk(b"gAMA", &[0, 0, 0xB1, 0x8F]),
            png_chunk(b"cHRM", &[7; 32]),
            png_chunk(b"iCCP", b"ICC profile\0\0a profile"),
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
    /// and with text (tEXt, zTXt, iTXt's XMP), tIME, eXIf, pHYs, sBIT, bKGD
    /// and a private chunk among them, and bytes after its IEND.
    pub(crate) fn laden_png() -> Vec<u8> {
        let (head, idat, iend) = png_parts();
        let (before, after) = png_kept();
        [
            head,
            png_chunk(b"tEXt", &[&b"Comment\0"[..], COMMENT].concat()),
            png_chunk(b"eXIf", &tiff(6)),
            before,
            png_chunk(b"zTXt", &[&b"Author\0\0"[..], NAME].concat()),
            png_chunk(b"pHYs", &[0, 0, 0x16, 0x25, 0, 0, 0x16, 0x25, 1]),
            png_chunk(b"sBIT", &[8, 8, 8]),
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
        let kept = strip(&laden).expect("kept");
        assert_eq!(identified(&kept), Vec::<&str>::new());
        let (head, idat, iend) = png_parts();
        let (before, after) = png_kept();
        assert_eq!(kept, [head, before, idat, after, iend].concat());
        assert_eq!(strip(&kept), Ok(kept.clone()), "rewritten to itself");
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
    /// Exif and XMP; the profile, its alpha and its image; then EXIF, `XMP `
    /// and a private chunk; and bytes after its RIFF.
    pub(crate) fn laden_webp() -> Vec<u8> {
        let (mut vp8x, alph, vp8) = alpha_parts();
        vp8x[0] |= 0x20 | VP8X_EXIF | VP8X_XMP;
        riff_of(
            &[
                riff_chunk(b"VP8X", &vp8x),
                riff_chunk(b"ICCP", b"a profile"),
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
        assert_eq!(identified(&laden).len(), 6, "{:?}", identified(&laden));
        let kept = strip(&laden).expect("kept");
        assert_eq!(identified(&kept), Vec::<&str>::new());
        let (mut vp8x, alph, vp8) = alpha_parts();
        vp8x[0] |= 0x20; // the ICC profile's flag, kept; Exif's and XMP's cleared
        let expected = riff_of(
            &[
                riff_chunk(b"VP8X", &vp8x),
                riff_chunk(b"ICCP", b"a profile"),
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
