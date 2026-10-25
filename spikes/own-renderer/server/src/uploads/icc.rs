//! **An ICC profile kept for its colour, without what names a device, a
//! person or a time** (track `upload-metadata`, ADR-XXXX: the independent
//! review of 2026-10-25, finding 1).
//!
//! An image's embedded profile is what its colours mean, and a browser
//! renders by it; but a screenshot's is its display's: its description
//! names the display's model, its maker's own tag may hold the display's
//! serial number, and its header says when it was made. A profile is read
//! as ICC.1 lays it out, in version 2 (ICC.1:2001-04) as in version 4
//! (ICC.1:2022), and in the versions before, which lay it out alike: a
//! header of 128 bytes; a tag table, its count and then twelve bytes a tag
//! (its signature, its data's offset from the profile's start, its data's
//! size); then each tag's data, its type's signature and four reserved
//! bytes first. What is kept is rebuilt from it:
//!
//! - **the header**, its size rewritten, and zeroed: when it was created
//!   (bytes 24 to 35), the device's maker, model and attributes (48 to 63),
//!   the profile's ID (84 to 99, an MD5 of the profile as it was) and the
//!   reserved bytes (100 to 127); its class, colour space, connection
//!   space, version, rendering intent and illuminant as they were;
//! - **each tag of a type of colour or rendering data**, byte for byte:
//!   `XYZ `, `curv`, `para`, `sf32`, `mft1`, `mft2`, `mAB `, `mBA `,
//!   `mpet`, `chrm`, `clro`, `meas`, `sig `, `view` and `cicp`;
//! - **each tag of a text type** (`desc`, `mluc` and `text`: the profile's
//!   description, the device's maker and model, the copyright, the viewing
//!   conditions' description, ...), as its type holding one fixed text,
//!   [`NEUTRAL`];
//! - and **no tag of any other type**: Apple's `mmod` (a display's maker,
//!   model and serial number), `dtim` (when it was calibrated), `dict`,
//!   `pseq`, `psid`, `data`, `ncl2`, `clrt`, `vcgt`, and a writer's own.
//!   Colour is computed from none of them.
//!
//! Tags that shared their data share it still, and the data is laid out in
//! its order, so what is kept is rebuilt to itself. A profile that does not
//! parse so (it is shorter than its header says, has no `acsp`, is of a
//! version over 4, has a tag outside it, or two tags' data overlapping but
//! not the same, or is over [`MAX`] bytes) is not kept at all, and a
//! browser shows its image as sRGB.

use super::sniff::be32;

/// **The most bytes a profile kept may have**: 4 MiB, this rewrite's own
/// bound on what a profile may cost to read (a PNG's is inflated to be
/// read). The profiles phones and screens embed are of a few hundred bytes
/// to a few kilobytes; a printer's, with large tables, of a few megabytes.
pub(crate) const MAX: usize = 4 << 20;

/// The most tags a profile kept may have.
const MAX_TAGS: usize = 1024;

/// A profile's header (ICC.1, 7.2): 128 bytes, then its tag table.
const HEADER: usize = 128;

/// **The text a text-bearing tag is given**, and a PNG's iCCP its name.
pub(crate) const NEUTRAL: &[u8] = b"ICC profile";

/// The header's bytes zeroed: when it was created (24 to 35); the device's
/// maker, model and attributes (48 to 63); the profile's ID and the
/// reserved bytes (84 to 127).
const ZEROED: [(usize, usize); 3] = [(24, 36), (48, 64), (84, 128)];

/// What a tag's type keeps of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Keeps {
    /// Every byte.
    Whole,
    /// Its type, holding [`NEUTRAL`].
    Neutral,
    /// Nothing: the tag is dropped.
    Nothing,
}

/// **What the tag of type `ty` keeps.**
fn keeps(ty: &[u8]) -> Keeps {
    match ty {
        b"XYZ " | b"curv" | b"para" | b"sf32" | b"mft1" | b"mft2" | b"mAB " | b"mBA " | b"mpet"
        | b"chrm" | b"clro" | b"meas" | b"sig " | b"view" | b"cicp" => Keeps::Whole,
        b"desc" | b"mluc" | b"text" => Keeps::Neutral,
        _ => Keeps::Nothing,
    }
}

/// **A text type `ty` holding [`NEUTRAL`] alone**, its type's signature and
/// four reserved bytes first.
fn neutral(ty: &[u8]) -> Vec<u8> {
    let mut t = [ty, &[0; 4]].concat();
    match ty {
        // textDescriptionType (ICC.1:2001-04, 6.5.17): the ASCII text's
        // count, its NUL counted, and the text; no Unicode text (its
        // language code and its count 0); no ScriptCode text (its code and
        // its count 0, and its 67 bytes).
        b"desc" => {
            t.extend((NEUTRAL.len() as u32 + 1).to_be_bytes());
            t.extend(NEUTRAL);
            t.extend([0; 1 + 4 + 4 + 2 + 1 + 67]);
        }
        // multiLocalizedUnicodeType (ICC.1:2022, 10.15): one record of 12
        // bytes (English, for the United States; its text's length and
        // offset), then its text, UTF-16BE.
        b"mluc" => {
            t.extend(1u32.to_be_bytes());
            t.extend(12u32.to_be_bytes());
            t.extend(b"enUS");
            t.extend((2 * NEUTRAL.len() as u32).to_be_bytes());
            t.extend(28u32.to_be_bytes());
            t.extend(NEUTRAL.iter().flat_map(|&c| [0, c]));
        }
        // textType (ICC.1:2022, 10.24): the text, and its NUL.
        _ => {
            t.extend(NEUTRAL);
            t.push(0);
        }
    }
    t
}

/// A tag of a profile's table: its signature, and its data's offset and
/// size.
struct Tag {
    sig: [u8; 4],
    at: usize,
    len: usize,
}

/// **`profile` rebuilt without what names a device, a person or a time**,
/// or nothing where it does not parse as an ICC profile.
pub(crate) fn scrub(profile: &[u8]) -> Option<Vec<u8>> {
    let size = be32(profile, 0)? as usize;
    if size < HEADER + 4 || size > profile.len() || size > MAX {
        return None;
    }
    // Bytes past the size its header says are not the profile's.
    let p = &profile[..size];
    if &p[36..40] != b"acsp" || p[8] > 4 {
        return None;
    }
    let count = be32(p, HEADER)? as usize;
    if count > MAX_TAGS {
        return None;
    }
    let table = HEADER + 4 + 12 * count;
    let mut tags = Vec::with_capacity(count);
    for entry in (HEADER + 4..table).step_by(12) {
        let (at, len) = (be32(p, entry + 4)? as usize, be32(p, entry + 8)? as usize);
        // After the table, within the profile, and its type's signature in it.
        if at < table || len < 4 || at.checked_add(len)? > size {
            return None;
        }
        let sig = p[entry..entry + 4].try_into().ok()?;
        tags.push(Tag { sig, at, len });
    }
    // Each tag's data is another's, the same bytes, or apart from all.
    let mut ranges: Vec<(usize, usize)> = tags.iter().map(|t| (t.at, t.len)).collect();
    ranges.sort_unstable();
    ranges.dedup();
    if ranges.windows(2).any(|w| w[0].0 + w[0].1 > w[1].0) {
        return None;
    }
    let kept: Vec<&Tag> = tags
        .iter()
        .filter(|t| keeps(&p[t.at..t.at + 4]) != Keeps::Nothing)
        .collect();
    let table = HEADER + 4 + 12 * kept.len();
    let mut out = Vec::with_capacity(size);
    out.extend(&p[..HEADER]);
    out.resize(table, 0);
    // Each datum where it is kept, in its order, four-byte aligned.
    let mut moved = Vec::with_capacity(ranges.len());
    for &(at, len) in &ranges {
        let ty = &p[at..at + 4];
        let keeps = keeps(ty);
        if keeps == Keeps::Nothing {
            moved.push((0, 0));
            continue;
        }
        out.resize(out.len().next_multiple_of(4), 0);
        let start = out.len();
        match keeps {
            Keeps::Whole => out.extend(&p[at..at + len]),
            _ => out.extend(neutral(ty)),
        }
        moved.push((start, out.len() - start));
    }
    out.resize(out.len().next_multiple_of(4), 0);
    // A profile that a text has made larger than any kept is not kept, so
    // what is kept is kept again.
    if out.len() > MAX {
        return None;
    }
    for (n, tag) in kept.iter().enumerate() {
        let (at, len) = moved[ranges.binary_search(&(tag.at, tag.len)).ok()?];
        let entry = HEADER + 4 + 12 * n;
        out[entry..entry + 4].copy_from_slice(&tag.sig);
        out[entry + 4..entry + 8].copy_from_slice(&(at as u32).to_be_bytes());
        out[entry + 8..entry + 12].copy_from_slice(&(len as u32).to_be_bytes());
    }
    let total = out.len() as u32;
    out[..4].copy_from_slice(&total.to_be_bytes());
    out[HEADER..HEADER + 4].copy_from_slice(&(kept.len() as u32).to_be_bytes());
    for (from, to) in ZEROED {
        out[from..to].fill(0);
    }
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::uploads::strip::tests::{
        CAMERA, COMMENT, CREATED, NAME, PROFILE_ID, SERIAL, identified,
    };

    /// An s15Fixed16Number (ICC.1, 4.6).
    fn s15(v: f64) -> [u8; 4] {
        ((v * 65536.0).round() as i32).to_be_bytes()
    }

    /// An XYZType of one XYZNumber.
    fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
        [&b"XYZ \0\0\0\0"[..], &s15(x), &s15(y), &s15(z)].concat()
    }

    /// The PCS's illuminant, D50 (ICC.1, 7.2.16).
    const D50: (f64, f64, f64) = (0.9642, 1.0, 0.8249);

    /// A text type of version `version` holding `text`: a version 2
    /// `desc` or `text`, or version 4's `mluc` (English, its text
    /// UTF-16BE).
    fn text_of(version: u8, ty: &[u8; 4], text: &[u8]) -> Vec<u8> {
        let mut t = if version >= 4 { b"mluc" } else { ty }.to_vec();
        t.extend([0; 4]);
        if version >= 4 {
            t.extend([&1u32.to_be_bytes()[..], &12u32.to_be_bytes(), b"enUS"].concat());
            t.extend((2 * text.len() as u32).to_be_bytes());
            t.extend(28u32.to_be_bytes());
            t.extend(text.iter().flat_map(|&c| [0, c]));
        } else if ty == b"desc" {
            t.extend((text.len() as u32 + 1).to_be_bytes());
            t.extend(text);
            t.extend([0; 1 + 4 + 4 + 2 + 1 + 67]);
        } else {
            t.extend(text);
            t.push(0);
        }
        t
    }

    /// **The colour tags of a display's profile**: its white (D50), sRGB's
    /// primaries adapted to D50, an identity adaptation, and one curve its
    /// three channels share: linear, so an engine that renders by the
    /// profile shows a stored mid grey lighter (gamma 1 in a version 2
    /// `curv`, or a version 4 `para`; or, with `points` over 1, a `curv`
    /// table of that many points, linear too).
    pub(crate) fn colour(version: u8, points: u32) -> Vec<(Vec<[u8; 4]>, Vec<u8>)> {
        let curve = if points > 1 {
            let mut c = [&b"curv\0\0\0\0"[..], &points.to_be_bytes()].concat();
            let last = (points - 1) as u64;
            for i in 0..points as u64 {
                c.extend(((i * 65535 / last) as u16).to_be_bytes());
            }
            c
        } else if version >= 4 {
            [&b"para\0\0\0\0\0\0\0\0"[..], &s15(1.0)].concat()
        } else {
            b"curv\0\0\0\0\0\0\0\x01\x01\x00".to_vec()
        };
        let mut chad = b"sf32\0\0\0\0".to_vec();
        for v in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
            chad.extend(s15(v));
        }
        vec![
            (vec![*b"wtpt"], xyz(D50.0, D50.1, D50.2)),
            (vec![*b"rXYZ"], xyz(0.4361, 0.2225, 0.0139)),
            (vec![*b"gXYZ"], xyz(0.3851, 0.7169, 0.0971)),
            (vec![*b"bXYZ"], xyz(0.1431, 0.0606, 0.7141)),
            (vec![*b"rTRC", *b"gTRC", *b"bTRC"], curve),
            (vec![*b"chad"], chad),
        ]
    }

    /// **What names a person, a device or a time, as a display's profile
    /// holds it**: its description (a person's name), its device's maker
    /// (a camera's name, as a phone's is) and model (with a serial number),
    /// its copyright (a comment), Apple's `mmod` (its maker, model and
    /// serial number), and when it was calibrated (`calt`, `dtim`).
    pub(crate) fn naming(version: u8) -> Vec<(Vec<[u8; 4]>, Vec<u8>)> {
        let model = [CAMERA, b" ", SERIAL].concat();
        let mmod = [&b"mmod\0\0\0\0PLRSP7PR"[..], SERIAL, &[0; 3]].concat();
        vec![
            (vec![*b"desc"], text_of(version, b"desc", NAME)),
            (vec![*b"dmnd"], text_of(version, b"desc", CAMERA)),
            (vec![*b"dmdd"], text_of(version, b"desc", &model)),
            (vec![*b"cprt"], text_of(version, b"text", COMMENT)),
            (vec![*b"mmod"], mmod),
            (vec![*b"calt"], [&b"dtim\0\0\0\0"[..], &CREATED].concat()),
        ]
    }

    /// **A profile of `tags`**, version `version`, a display's (`mntr`,
    /// `RGB `, `XYZ `): its header's creation time [`CREATED`], its
    /// device's maker `PLRS`, model `P7PR` and attributes `DEADBEEF`, its
    /// ID [`PROFILE_ID`]; then each tag's data in order after the table,
    /// four-byte aligned, once for all its signatures.
    pub(crate) fn profile(version: u8, tags: &[(Vec<[u8; 4]>, Vec<u8>)]) -> Vec<u8> {
        let count: usize = tags.iter().map(|(sigs, _)| sigs.len()).sum();
        let mut p = vec![0; HEADER];
        p[8] = version;
        p[9] = if version >= 4 { 0x30 } else { 0x10 };
        p[12..24].copy_from_slice(b"mntrRGB XYZ ");
        p[24..36].copy_from_slice(&CREATED);
        p[36..44].copy_from_slice(b"acspAPPL");
        p[48..60].copy_from_slice(b"PLRSP7PR\xDE\xAD\xBE\xEF");
        p[68..80].copy_from_slice(&[s15(D50.0), s15(D50.1), s15(D50.2)].concat());
        p[80..84].copy_from_slice(b"PLRS");
        p[84..100].copy_from_slice(PROFILE_ID);
        p.extend((count as u32).to_be_bytes());
        p.resize(HEADER + 4 + 12 * count, 0);
        let mut entry = HEADER + 4;
        for (sigs, data) in tags {
            p.resize(p.len().next_multiple_of(4), 0);
            let at = p.len() as u32;
            p.extend(data);
            for sig in sigs {
                p[entry..entry + 4].copy_from_slice(sig);
                p[entry + 4..entry + 8].copy_from_slice(&at.to_be_bytes());
                p[entry + 8..entry + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
                entry += 12;
            }
        }
        p.resize(p.len().next_multiple_of(4), 0);
        let size = p.len() as u32;
        p[..4].copy_from_slice(&size.to_be_bytes());
        p
    }

    /// **A display's profile as a screenshot embeds it**, version 2 or 4:
    /// its colour tags, and what names a person, a device or a time.
    pub(crate) fn planted(version: u8) -> Vec<u8> {
        profile(version, &[naming(version), colour(version, 1)].concat())
    }

    /// A profile's tags, as its table lists them: each signature, and its
    /// data.
    pub(crate) fn tags_of(p: &[u8]) -> Vec<([u8; 4], &[u8])> {
        let count = be32(p, HEADER).unwrap() as usize;
        (0..count)
            .map(|n| {
                let e = HEADER + 4 + 12 * n;
                let (at, len) = (
                    be32(p, e + 4).unwrap() as usize,
                    be32(p, e + 8).unwrap() as usize,
                );
                (p[e..e + 4].try_into().unwrap(), &p[at..at + len])
            })
            .collect()
    }

    /// **What a profile kept says**: no name, device or time; its header's
    /// colour as it was; each colour tag byte for byte, the three curves
    /// still one; each text a fixed text of its type; no `mmod` or `calt`;
    /// and it is rebuilt to itself.
    #[test]
    fn a_profile_keeps_its_colour_and_names_nothing() {
        for version in [2, 4] {
            let planted = planted(version);
            assert_eq!(
                identified(&planted),
                [
                    "a camera",
                    "a person's name",
                    "a comment",
                    "an ICC profile's creation time",
                    "an ICC profile's ID",
                    "a serial number"
                ],
                "version {version}"
            );
            let kept = scrub(&planted).expect("kept");
            assert_eq!(identified(&kept), Vec::<&str>::new(), "version {version}");
            // The header: its size, and zeros where it named a device or a time.
            assert_eq!(be32(&kept, 0), Some(kept.len() as u32));
            let mut header = planted[..HEADER].to_vec();
            header[..4].copy_from_slice(&(kept.len() as u32).to_be_bytes());
            for (from, to) in ZEROED {
                header[from..to].fill(0);
            }
            assert_eq!(kept[..HEADER], header[..]);
            assert_eq!(
                &kept[48..64],
                &[0; 16],
                "no device's maker, model or attributes"
            );
            // Its tags: the colour's whole, the texts' fixed, no others.
            let before = tags_of(&planted);
            let after = tags_of(&kept);
            let sigs: Vec<&[u8]> = after.iter().map(|(s, _)| &s[..]).collect();
            assert_eq!(
                sigs,
                [
                    "desc", "dmnd", "dmdd", "cprt", "wtpt", "rXYZ", "gXYZ", "bXYZ", "rTRC", "gTRC",
                    "bTRC", "chad"
                ]
                .map(str::as_bytes)
            );
            for (sig, data) in &after {
                let (_, was) = before.iter().find(|(s, _)| s == sig).unwrap();
                if sig.starts_with(b"d") || sig == b"cprt" {
                    assert_eq!(&data[..4], &was[..4], "of its type");
                    assert_eq!(
                        *data,
                        neutral(&was[..4]),
                        "{}",
                        String::from_utf8_lossy(sig)
                    );
                } else {
                    assert_eq!(data, was, "{} byte for byte", String::from_utf8_lossy(sig));
                }
            }
            let curves = |p: &[u8]| -> Vec<Vec<u8>> {
                let table = &p[HEADER + 4..];
                (0..be32(p, HEADER).unwrap() as usize)
                    .filter(|n| table[12 * n + 1..12 * n + 4] == *b"TRC")
                    .map(|n| table[12 * n + 4..12 * n + 12].to_vec())
                    .collect()
            };
            let shared = curves(&kept);
            assert!(
                shared.len() == 3 && shared.iter().all(|c| *c == shared[0]),
                "one curve"
            );
            assert_eq!(scrub(&kept), Some(kept.clone()), "rebuilt to itself");
        }
    }

    /// **The fixed text, as each type holds it**, byte for byte.
    #[test]
    fn a_text_tag_holds_one_fixed_text_of_its_type() {
        let text = b"text\0\0\0\0ICC profile\0";
        assert_eq!(neutral(b"text"), text);
        let mut desc = b"desc\0\0\0\0\0\0\0\x0cICC profile\0".to_vec();
        desc.extend([0; 4 + 4 + 2 + 1 + 67]);
        assert_eq!(neutral(b"desc"), desc);
        assert_eq!(desc.len(), 102);
        let mut mluc = b"mluc\0\0\0\0\0\0\0\x01\0\0\0\x0cenUS\0\0\0\x16\0\0\0\x1c".to_vec();
        mluc.extend(b"ICC profile".iter().flat_map(|&c| [0, c]));
        assert_eq!(neutral(b"mluc"), mluc);
        // The tests' own texts are laid out alike.
        assert_eq!(text_of(2, b"desc", NEUTRAL), neutral(b"desc"));
        assert_eq!(text_of(2, b"text", NEUTRAL), neutral(b"text"));
        assert_eq!(text_of(4, b"desc", NEUTRAL), neutral(b"mluc"));
    }

    /// **A profile that does not parse is not kept at all**, nor one that
    /// a text would make larger than [`MAX`]; bytes past the size a
    /// profile's header says are not kept with it.
    #[test]
    fn a_profile_that_does_not_parse_is_not_kept() {
        let planted = planted(2);
        let kept = scrub(&planted).expect("kept");
        let changed = |at: usize, bytes: &[u8]| {
            let mut p = planted.clone();
            p[at..at + bytes.len()].copy_from_slice(bytes);
            scrub(&p)
        };
        let entry = |n: usize| HEADER + 4 + 12 * n;
        let (desc_at, desc_len) = (
            be32(&planted, entry(0) + 4).unwrap(),
            be32(&planted, entry(0) + 8).unwrap(),
        );
        let wtpt_at = be32(&planted, entry(6) + 4).unwrap();
        assert_eq!(&planted[entry(6)..entry(6) + 4], b"wtpt");
        let cases: [(&str, Option<Vec<u8>>); 10] = [
            (
                "shorter than its header and table",
                scrub(&planted[..HEADER + 3]),
            ),
            (
                "shorter than its size says",
                scrub(&planted[..planted.len() - 1]),
            ),
            (
                "its size under its header's",
                changed(0, &131u32.to_be_bytes()),
            ),
            ("no `acsp`", changed(36, b"acsP")),
            ("version 5, iccMAX's", changed(8, &[5])),
            (
                "more tags than its table holds",
                changed(HEADER, &1000u32.to_be_bytes()),
            ),
            (
                "a tag past its end",
                changed(entry(6) + 8, &(planted.len() as u32).to_be_bytes()),
            ),
            (
                "a tag in its table",
                changed(entry(6) + 4, &(HEADER as u32 + 4).to_be_bytes()),
            ),
            (
                "a tag shorter than its type",
                changed(entry(6) + 8, &3u32.to_be_bytes()),
            ),
            (
                "a text over its white point's start",
                changed(entry(0) + 8, &(wtpt_at - desc_at + 4).to_be_bytes()),
            ),
        ];
        for (what, scrubbed) in cases {
            assert_eq!(scrubbed, None, "{what}");
        }
        assert!(
            desc_len < wtpt_at - desc_at,
            "the description ends before the white point"
        );
        // At most 1,024 tags, though they share one datum.
        let many = |n: usize| profile(2, &[(vec![*b"wtpt"; n], xyz(D50.0, D50.1, D50.2))]);
        assert!(scrub(&many(1024)).is_some(), "1,024 tags");
        assert_eq!(scrub(&many(1025)), None, "1,025 tags");
        // Two tags whose data is the same bytes are one datum: kept.
        assert!(changed(entry(1) + 4, &planted[entry(0) + 4..entry(0) + 12]).is_some());
        // What follows a profile is not the profile's.
        assert_eq!(scrub(&[&planted[..], b"Jane Roe"].concat()), Some(kept));
        // A profile of nearly the most bytes a profile kept may have, whose
        // short description the fixed text makes larger than that, is not
        // kept, so that what is kept is kept again; without that
        // description, it is kept.
        let points = (MAX as u32 - 396) / 2;
        let mut tags = colour(2, points);
        tags.push((vec![*b"desc"], b"desc\0\0\0\0\0\0\0\x01\0".to_vec()));
        let at_most = profile(2, &tags);
        assert_eq!(at_most.len(), MAX - 4);
        assert_eq!(scrub(&at_most), None);
        tags.pop();
        let fits = profile(2, &tags);
        assert_eq!(scrub(&fits).map(|p| p.len()), Some(fits.len()));
    }
}
