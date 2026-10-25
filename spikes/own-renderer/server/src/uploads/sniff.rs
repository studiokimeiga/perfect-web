//! **What an upload's bytes are, read from the bytes** (track `uploads`):
//! its type by its magic number, and its intrinsic width and height from its
//! header, never from the request's `Content-Type` or the file's name.
//!
//! Each reader is the format's own header, checked against its primary
//! source:
//!
//! - **The signatures**: the WHATWG MIME Sniffing Standard, §6.1 "Matching an
//!   image type pattern": `GIF87a` and `GIF89a`; `RIFF`, four bytes, and
//!   `WEBPVP`; `89 50 4E 47 0D 0A 1A 0A`; and `FF D8 FF`.
//! - **PNG** (W3C PNG, third edition): "The IHDR chunk shall be the first
//!   chunk", its data 13 bytes, width and height four-byte big-endian
//!   integers of which "Zero is an invalid value", at most 2³¹−1; its CRC
//!   over the chunk's type and data (ISO 3309, as §5.5 gives it).
//! - **JPEG** (ITU-T T.81, Annex B): a marker "may optionally be preceded by
//!   fill bytes X'FF'"; SOFn is `C0`–`C3`, `C5`–`C7`, `C9`–`CB`, `CD`–`CF`;
//!   a frame header's `Y` (lines) may be zero, defined later by DNL, which is
//!   refused here, and its `X` is not zero. Where an Exif APP1 segment says
//!   the image is turned a quarter (CIPA DC-008, `Orientation` 5 to 8), a
//!   browser shows it turned (CSS Images 3, `image-orientation: from-image`),
//!   so its width and height are swapped as it is shown.
//! - **WebP** (RFC 9649): `RIFF`, the size from offset 8, `WEBP`, then
//!   `VP8 ` (RFC 6386 §9.1: a key frame's tag, start code `9D 01 2A`, then
//!   two little-endian 16-bit fields, 14 bits of size and 2 of scale),
//!   `VP8L` (signature `2F`, then 14 bits of width − 1 and 14 of height − 1,
//!   and a version that "MUST be 0"), or `VP8X` (a flags byte, three
//!   reserved, then 24-bit canvas width − 1 and height − 1, whose product
//!   "MUST be at most 2³² − 1").
//! - **GIF** (GIF89a, §17 and §18): the header, then the logical screen
//!   descriptor's width and height, "Least Significant Byte first".

/// **The image types an upload may be**: each a magic number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Png,
    Jpeg,
    Webp,
    Gif,
}

impl Kind {
    /// Every kind, in the order a declaration lists them.
    pub const ALL: [Kind; 4] = [Kind::Png, Kind::Jpeg, Kind::Webp, Kind::Gif];

    /// Its media type, as the WHATWG standard computes it.
    pub fn mime(self) -> &'static str {
        match self {
            Kind::Png => "image/png",
            Kind::Jpeg => "image/jpeg",
            Kind::Webp => "image/webp",
            Kind::Gif => "image/gif",
        }
    }

    /// The word a declaration names it by, and its path's extension.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Png => "png",
            Kind::Jpeg => "jpeg",
            Kind::Webp => "webp",
            Kind::Gif => "gif",
        }
    }

    /// The kind a declaration's word names.
    pub fn named(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.word() == word)
    }
}

/// **What the bytes say they are**: the kind, and the width and height a
/// browser shows them at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
}

/// **Why bytes are not an image of a kind this reads.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// No signature this knows: not an image of an allowed kind.
    Unrecognized,
    /// A signature, and a header that does not hold what its format says.
    Malformed(Kind, &'static str),
}

/// **The kind the bytes' signature names**, and nothing else: the WHATWG
/// standard's patterns. A WebP's pattern is `RIFF`, four bytes, `WEBPVP`.
pub fn sniff(bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(Kind::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Kind::Jpeg)
    } else if bytes.len() >= 14 && &bytes[0..4] == b"RIFF" && &bytes[8..14] == b"WEBPVP" {
        Some(Kind::Webp)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Kind::Gif)
    } else {
        None
    }
}

/// **The bytes' kind, width and height**, each from the bytes.
pub fn measure(bytes: &[u8]) -> Result<Measured, Unreadable> {
    let kind = sniff(bytes).ok_or(Unreadable::Unrecognized)?;
    let bad = |why| Unreadable::Malformed(kind, why);
    let (width, height) = match kind {
        Kind::Png => png(bytes).map_err(bad)?,
        Kind::Jpeg => jpeg(bytes).map_err(bad)?,
        Kind::Webp => webp(bytes).map_err(bad)?,
        Kind::Gif => gif(bytes).map_err(bad)?,
    };
    Ok(Measured {
        kind,
        width,
        height,
    })
}

pub(crate) fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}

pub(crate) fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}

fn le24(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 3)?;
    Some(s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16)
}

pub(crate) fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// CRC-32 as PNG computes it (ISO 3309; PNG's Annex D): reflected
/// polynomial `EDB88320`, starting and ending inverted.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    c ^ 0xFFFF_FFFF
}

/// PNG: the signature, then IHDR first, 13 bytes, its CRC whole.
fn png(b: &[u8]) -> Result<(u32, u32), &'static str> {
    const SHORT: &str = "it ends before its IHDR chunk does";
    let length = be32(b, 8).ok_or(SHORT)?;
    if b.get(12..16) != Some(b"IHDR") {
        return Err("its first chunk is not IHDR");
    }
    if length != 13 {
        return Err("its IHDR chunk is not 13 bytes");
    }
    let crc = be32(b, 29).ok_or(SHORT)?;
    if crc32(&b[12..29]) != crc {
        return Err("its IHDR chunk's CRC does not match");
    }
    let (w, h) = (be32(b, 16).ok_or(SHORT)?, be32(b, 20).ok_or(SHORT)?);
    if w == 0 || h == 0 {
        return Err("its width or height is zero");
    }
    if w > i32::MAX as u32 || h > i32::MAX as u32 {
        return Err("its width or height is over 2³¹−1");
    }
    Ok((w, h))
}

/// GIF: the header, then the logical screen's width and height.
fn gif(b: &[u8]) -> Result<(u32, u32), &'static str> {
    const SHORT: &str = "it ends before its logical screen descriptor does";
    if b.len() < 13 {
        return Err(SHORT);
    }
    let (w, h) = (le16(b, 6).ok_or(SHORT)?, le16(b, 8).ok_or(SHORT)?);
    if w == 0 || h == 0 {
        return Err("its logical screen's width or height is zero");
    }
    Ok((w, h))
}

/// WebP: the RIFF header, then its first chunk, `VP8 `, `VP8L` or `VP8X`.
fn webp(b: &[u8]) -> Result<(u32, u32), &'static str> {
    const SHORT: &str = "it ends before its first chunk's header does";
    let riff = le32(b, 4).ok_or(SHORT)? as usize;
    // "the size of the file in bytes, starting at offset 8".
    if riff < 4 || riff.checked_add(8).is_none_or(|end| end > b.len()) {
        return Err("its RIFF size is not the bytes it has");
    }
    if b.get(8..12) != Some(b"WEBP") {
        return Err("its RIFF form is not WEBP");
    }
    let fourcc = b.get(12..16).ok_or(SHORT)?;
    let size = le32(b, 16).ok_or(SHORT)? as usize;
    let data = b
        .get(20..)
        .filter(|d| d.len() >= size)
        .map(|d| &d[..size])
        .ok_or("its first chunk is longer than its bytes")?;
    match fourcc {
        b"VP8 " => {
            // A key frame's tag: its first bit 0.
            if data.len() < 10 {
                return Err("its VP8 chunk ends before its frame header");
            }
            if data[0] & 1 != 0 {
                return Err("its VP8 frame is not a key frame");
            }
            if data[3..6] != [0x9D, 0x01, 0x2A] {
                return Err("its VP8 key frame has no start code");
            }
            let w = le16(data, 6).ok_or(SHORT)? & 0x3FFF;
            let h = le16(data, 8).ok_or(SHORT)? & 0x3FFF;
            if w == 0 || h == 0 {
                return Err("its VP8 frame's width or height is zero");
            }
            Ok((w, h))
        }
        b"VP8L" => {
            if data.len() < 5 {
                return Err("its VP8L chunk ends before its header");
            }
            if data[0] != 0x2F {
                return Err("its VP8L chunk has no signature");
            }
            let bits = le32(data, 1).ok_or(SHORT)?;
            if bits >> 29 != 0 {
                return Err("its VP8L version is not 0");
            }
            Ok(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => {
            if data.len() < 10 {
                return Err("its VP8X chunk is shorter than 10 bytes");
            }
            let w = le24(data, 4).ok_or(SHORT)? + 1;
            let h = le24(data, 7).ok_or(SHORT)? + 1;
            if (w as u64) * (h as u64) > u32::MAX as u64 {
                return Err("its canvas's width times height is over 2³²−1");
            }
            Ok((w, h))
        }
        _ => Err("its first chunk is not VP8, VP8L or VP8X"),
    }
}

/// Is `m` a start-of-frame marker (T.81 Table B.1)?
fn sof(m: u8) -> bool {
    matches!(m, 0xC0..=0xCF) && !matches!(m, 0xC4 | 0xC8 | 0xCC)
}

/// JPEG: the segments from SOI to the first frame header, its `X` and `Y`,
/// turned as an Exif `Orientation` before it turns them.
fn jpeg(b: &[u8]) -> Result<(u32, u32), &'static str> {
    const SHORT: &str = "it ends before its frame header";
    let mut at = 2; // past SOI
    let mut turned = false;
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
            // TEM and RSTm stand alone.
            0x01 | 0xD0..=0xD7 => continue,
            0xD8 => return Err("a second SOI before its frame header"),
            0xD9 => return Err("it ends (EOI) before its frame header"),
            0xDA => return Err("its scan (SOS) comes before its frame header"),
            0x00 => return Err("a stuffed zero where a marker is"),
            _ => {}
        }
        let length = be16(b, at).ok_or(SHORT)? as usize;
        if length < 2 {
            return Err("a segment's length is under 2");
        }
        let segment = b.get(at + 2..at + length).ok_or(SHORT)?;
        if sof(m) {
            // Lf, then P (1), Y (2), X (2).
            let y = be16(segment, 1).ok_or(SHORT)?;
            let x = be16(segment, 3).ok_or(SHORT)?;
            if x == 0 {
                return Err("its frame's samples per line (X) is zero");
            }
            if y == 0 {
                return Err("its frame's lines (Y) is zero, defined later by DNL");
            }
            return Ok(if turned { (y, x) } else { (x, y) });
        }
        if m == 0xE1 && segment.starts_with(b"Exif\0\0") {
            turned = matches!(orientation(&segment[6..]), Some(5..=8));
        }
        at += length;
    }
}

/// **An Exif TIFF structure's `Orientation`** (CIPA DC-008, tag `0112`, a
/// SHORT in IFD0), where it has one it can read. A structure it cannot read
/// says nothing, as a browser that cannot read it shows the image unturned.
/// A reader of an unsigned integer at an offset, in one byte order.
type Reader = fn(&[u8], usize) -> Option<u32>;

pub(crate) fn orientation(tiff: &[u8]) -> Option<u32> {
    let (r16, r32): (Reader, Reader) = match tiff.get(0..2)? {
        b"II" => (le16, le32),
        b"MM" => (be16, be32),
        _ => return None,
    };
    if r16(tiff, 2)? != 42 {
        return None;
    }
    let ifd = r32(tiff, 4)? as usize;
    let count = r16(tiff, ifd)? as usize;
    for i in 0..count {
        let entry = ifd.checked_add(2 + 12 * i)?;
        if r16(tiff, entry)? == 0x0112 && r16(tiff, entry + 2)? == 3 && r32(tiff, entry + 4)? == 1 {
            return r16(tiff, entry + 8);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG's signature and IHDR, its CRC computed, then nothing.
    pub(crate) fn png_head(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        b.extend(13u32.to_be_bytes());
        let mut chunk = b"IHDR".to_vec();
        chunk.extend(w.to_be_bytes());
        chunk.extend(h.to_be_bytes());
        chunk.extend([8, 6, 0, 0, 0]);
        let crc = crc32(&chunk);
        b.extend(&chunk);
        b.extend(crc.to_be_bytes());
        b
    }

    fn gif_head(version: &[u8; 6], w: u16, h: u16) -> Vec<u8> {
        let mut b = version.to_vec();
        b.extend(w.to_le_bytes());
        b.extend(h.to_le_bytes());
        b.extend([0, 0, 0]);
        b
    }

    fn riff(chunk: &[u8], data: &[u8]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        body.extend(chunk);
        body.extend((data.len() as u32).to_le_bytes());
        body.extend(data);
        let mut b = b"RIFF".to_vec();
        b.extend((body.len() as u32).to_le_bytes());
        b.extend(body);
        b
    }

    fn vp8(w: u16, h: u16) -> Vec<u8> {
        let mut d = vec![0x10, 0x02, 0x00, 0x9D, 0x01, 0x2A];
        d.extend(w.to_le_bytes());
        d.extend(h.to_le_bytes());
        d.extend([0; 4]);
        d
    }

    fn vp8l(w: u32, h: u32) -> Vec<u8> {
        let bits = (w - 1) | (h - 1) << 14;
        let mut d = vec![0x2F];
        d.extend(bits.to_le_bytes());
        d
    }

    fn vp8x(w: u32, h: u32) -> Vec<u8> {
        let mut d = vec![0x10, 0, 0, 0];
        d.extend(&(w - 1).to_le_bytes()[..3]);
        d.extend(&(h - 1).to_le_bytes()[..3]);
        d
    }

    /// SOI, the segments, SOF0 for `x` by `y`, and EOI.
    fn jpeg_with(segments: &[(u8, Vec<u8>)], x: u16, y: u16) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        for (m, data) in segments {
            b.extend([0xFF, *m]);
            b.extend(((data.len() + 2) as u16).to_be_bytes());
            b.extend(data);
        }
        b.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend(y.to_be_bytes());
        b.extend(x.to_be_bytes());
        b.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        b.extend([0xFF, 0xD9]);
        b
    }

    fn exif(order: &[u8; 2], orientation: u16) -> Vec<u8> {
        let big = order == b"MM";
        let u16b = |v: u16| {
            if big {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let u32b = |v: u32| {
            if big {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let mut d = b"Exif\0\0".to_vec();
        d.extend(order);
        d.extend(u16b(42));
        d.extend(u32b(8));
        d.extend(u16b(1));
        d.extend(u16b(0x0112));
        d.extend(u16b(3));
        d.extend(u32b(1));
        d.extend(u16b(orientation));
        d.extend([0, 0]);
        d.extend(u32b(0));
        d
    }

    fn measured(kind: Kind, width: u32, height: u32) -> Result<Measured, Unreadable> {
        Ok(Measured {
            kind,
            width,
            height,
        })
    }

    #[test]
    fn crc32_is_iso_3309s() {
        // The check value of CRC-32/ISO-HDLC, the CRC PNG uses.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn a_png_is_its_ihdrs_width_and_height() {
        assert_eq!(measure(&png_head(640, 480)), measured(Kind::Png, 640, 480));
        assert_eq!(
            measure(&png_head(1, 70_000)),
            measured(Kind::Png, 1, 70_000)
        );
    }

    #[test]
    fn a_png_whose_ihdr_is_wrong_is_refused() {
        let ok = png_head(640, 480);
        let mut crc = ok.clone();
        crc[17] ^= 1; // the width changed, the CRC not
        assert_eq!(
            measure(&crc),
            Err(Unreadable::Malformed(
                Kind::Png,
                "its IHDR chunk's CRC does not match"
            ))
        );
        assert!(measure(&ok[..30]).is_err(), "cut in its CRC");
        assert!(measure(&png_head(0, 4)).is_err(), "zero wide");
        assert!(measure(&png_head(4, 0)).is_err(), "zero high");
        assert!(measure(&png_head(1 << 31, 4)).is_err(), "over 2³¹−1");
        let mut first = ok.clone();
        first[12..16].copy_from_slice(b"IDAT");
        assert!(measure(&first).is_err(), "IHDR not first");
        let mut long = ok;
        long[11] = 14;
        assert!(measure(&long).is_err(), "IHDR not 13 bytes");
    }

    #[test]
    fn a_gif_is_its_logical_screens_width_and_height() {
        assert_eq!(
            measure(&gif_head(b"GIF89a", 300, 2)),
            measured(Kind::Gif, 300, 2)
        );
        assert_eq!(
            measure(&gif_head(b"GIF87a", 2, 300)),
            measured(Kind::Gif, 2, 300)
        );
        assert!(measure(&gif_head(b"GIF89a", 0, 3)).is_err());
        assert!(measure(&gif_head(b"GIF89a", 3, 3)[..12]).is_err());
        assert_eq!(sniff(&gif_head(b"GIF88a", 3, 3)), None, "no such version");
    }

    #[test]
    fn a_webp_is_its_first_chunks_width_and_height() {
        assert_eq!(
            measure(&riff(b"VP8 ", &vp8(321, 123))),
            measured(Kind::Webp, 321, 123)
        );
        // The two scale bits are not the size.
        assert_eq!(
            measure(&riff(b"VP8 ", &vp8(0xC000 | 321, 0x4000 | 123))),
            measured(Kind::Webp, 321, 123)
        );
        assert_eq!(
            measure(&riff(b"VP8L", &vp8l(16384, 1))),
            measured(Kind::Webp, 16384, 1)
        );
        assert_eq!(
            measure(&riff(b"VP8L", &vp8l(5, 7))),
            measured(Kind::Webp, 5, 7)
        );
        assert_eq!(
            measure(&riff(b"VP8X", &vp8x(70_000, 3))),
            measured(Kind::Webp, 70_000, 3)
        );
    }

    #[test]
    fn a_webp_whose_header_is_wrong_is_refused() {
        let mut inter = vp8(4, 4);
        inter[0] |= 1;
        assert!(measure(&riff(b"VP8 ", &inter)).is_err(), "not a key frame");
        let mut code = vp8(4, 4);
        code[5] = 0x2B;
        assert!(measure(&riff(b"VP8 ", &code)).is_err(), "no start code");
        let mut sig = vp8l(4, 4);
        sig[0] = 0x2E;
        assert!(measure(&riff(b"VP8L", &sig)).is_err(), "no VP8L signature");
        let mut version = vp8l(4, 4);
        version[4] |= 0x20;
        assert!(measure(&riff(b"VP8L", &version)).is_err(), "VP8L version 1");
        assert!(
            measure(&riff(b"VP8X", &vp8x(1 << 24, 1 << 24))).is_err(),
            "a canvas over 2³²−1"
        );
        let whole = riff(b"VP8L", &vp8l(4, 4));
        assert!(
            measure(&whole[..whole.len() - 1]).is_err(),
            "RIFF size past its end"
        );
        // `WEBPVP`, its sniffing pattern, and an unknown chunk after `VP`.
        assert!(measure(&riff(b"VP8Z", &vp8l(4, 4))).is_err());
    }

    #[test]
    fn a_jpeg_is_its_first_frame_headers_width_and_height() {
        let jfif = (0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0".to_vec());
        assert_eq!(
            measure(&jpeg_with(std::slice::from_ref(&jfif), 640, 480)),
            measured(Kind::Jpeg, 640, 480)
        );
        // Fill bytes before a marker, and a progressive frame (SOF2).
        let mut filled = jpeg_with(&[jfif], 8, 6);
        let sof_at = filled
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("SOF0");
        filled[sof_at + 1] = 0xC2;
        filled.splice(sof_at..sof_at, [0xFF, 0xFF]);
        assert_eq!(measure(&filled), measured(Kind::Jpeg, 8, 6));
    }

    #[test]
    fn a_jpeg_turned_a_quarter_is_as_tall_as_it_is_wide() {
        for order in [b"II", b"MM"] {
            for (o, turned) in [(1, false), (3, false), (5, true), (6, true), (8, true)] {
                let b = jpeg_with(&[(0xE1, exif(order, o))], 40, 30);
                let (w, h) = if turned { (30, 40) } else { (40, 30) };
                assert_eq!(measure(&b), measured(Kind::Jpeg, w, h), "orientation {o}");
            }
        }
        // An Exif structure it cannot read says nothing.
        let mut broken = exif(b"II", 6);
        broken[8] = 41;
        assert_eq!(
            measure(&jpeg_with(&[(0xE1, broken)], 40, 30)),
            measured(Kind::Jpeg, 40, 30)
        );
    }

    #[test]
    fn a_jpeg_without_a_frame_header_is_refused() {
        let b = jpeg_with(&[], 640, 480);
        let sof_at = b.windows(2).position(|w| w == [0xFF, 0xC0]).expect("SOF0");
        assert!(
            measure(&b[..sof_at + 6]).is_err(),
            "cut in its frame header"
        );
        let mut sos = b.clone();
        sos[sof_at + 1] = 0xDA;
        assert!(measure(&sos).is_err(), "a scan before any frame");
        let mut dht = b.clone();
        dht[sof_at + 1] = 0xC4; // DHT is not SOFn
        assert!(measure(&dht).is_err());
        assert!(
            measure(&jpeg_with(&[], 640, 0)).is_err(),
            "Y defined by DNL"
        );
        assert!(measure(&jpeg_with(&[], 0, 480)).is_err(), "X zero");
        let mut length = b;
        length[4] = 0;
        length[5] = 1;
        assert!(measure(&length).is_err(), "length under 2");
        assert!(measure(&[0xFF, 0xD8, 0xFF]).is_err());
    }

    #[test]
    fn the_kind_is_the_bytes_and_nothing_else() {
        assert_eq!(
            measure(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            Err(Unreadable::Unrecognized)
        );
        assert_eq!(
            measure(b"<!DOCTYPE html><script>alert(1)</script>"),
            Err(Unreadable::Unrecognized)
        );
        assert_eq!(measure(b""), Err(Unreadable::Unrecognized));
        // A PNG's signature with HTML after it is a PNG, and a malformed one.
        let mut polyglot = png_head(1, 1)[..8].to_vec();
        polyglot.extend(b"<html><script>alert(1)</script>");
        assert!(matches!(
            measure(&polyglot),
            Err(Unreadable::Malformed(Kind::Png, _))
        ));
    }

    /// **Real encoders' files** (`scripts/uploads_fixtures.py`, Pillow):
    /// each read as the kind, width and height the fixtures name, which is
    /// what a browser shows (the browser suite checks that too). Among them
    /// a lossy, a lossless and an extended WebP, a progressive JPEG, and a
    /// JPEG an Exif `Orientation` turns.
    #[test]
    fn real_encoders_files_are_read_as_they_are_shown() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../e2e/uploads");
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("fixtures.json")).unwrap())
                .unwrap();
        assert_eq!(rows.len(), 10);
        for row in rows {
            let file = row["file"].as_str().unwrap();
            let bytes = std::fs::read(dir.join(file)).unwrap();
            let read = measure(&bytes);
            match row["kind"].as_str() {
                Some(kind) => assert_eq!(
                    read,
                    measured(
                        Kind::named(kind).unwrap(),
                        row["width"].as_u64().unwrap() as u32,
                        row["height"].as_u64().unwrap() as u32
                    ),
                    "{file}"
                ),
                None => assert_eq!(read, Err(Unreadable::Unrecognized), "{file}"),
            }
        }
    }

    #[test]
    fn every_kind_is_named_by_its_word() {
        for k in Kind::ALL {
            assert_eq!(Kind::named(k.word()), Some(k));
            assert!(k.mime().starts_with("image/"));
        }
        assert_eq!(Kind::named("svg"), None);
    }
}
