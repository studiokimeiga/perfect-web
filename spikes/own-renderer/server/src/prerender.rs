//! **A page no reader decides is one document** (the prerender's ADR): the
//! host renders it once, before it listens, and sends everyone those bytes,
//! compressed and revalidated as a file of the build is. Which pages those
//! are is the compiler's (`pw_core::prerender`), which a plan says.
//!
//! One rendering, by the function every other document is rendered with,
//! held in this process: no file on disk can be another build's.

use sha2::Digest;

/// **One form of the document**: its bytes, what `content-encoding` says of
/// them, where anything does, and the strong validator that names them.
pub(crate) struct Representation {
    pub(crate) encoding: Option<&'static str>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) tag: String,
}

impl Representation {
    /// Its validator names `says`, what the document says of itself, too: a
    /// browser told it holds a document keeps the headers it holds with it,
    /// so a policy that changed, the page's bytes the same, is a new document.
    fn of(encoding: Option<&'static str>, bytes: Vec<u8>, says: &str) -> Representation {
        let tag = strong_tag(&[bytes.as_slice(), says.as_bytes()].concat());
        Representation {
            encoding,
            bytes,
            tag,
        }
    }
}

/// **A page no reader decides, as it is sent**: each representation made
/// once, and what every document says of itself (`Server::document_headers`).
pub(crate) struct Prerendered {
    br: Representation,
    gzip: Representation,
    identity: Representation,
    pub(crate) headers: String,
}

impl Prerendered {
    pub(crate) fn of(document: String, headers: String) -> Result<Prerendered, String> {
        let text = document.into_bytes();
        Ok(Prerendered {
            br: Representation::of(Some("br"), brotli_of(&text)?, &headers),
            gzip: Representation::of(Some("gzip"), gzip_of(&text), &headers),
            identity: Representation::of(None, text, &headers),
            headers,
        })
    }

    /// The representation a request accepts: Brotli, else gzip, else the
    /// document as it is. `accepts` is the request's answer for an encoding.
    pub(crate) fn accepted(&self, accepts: impl Fn(&str) -> bool) -> &Representation {
        if accepts("br") {
            &self.br
        } else if accepts("gzip") {
            &self.gzip
        } else {
            &self.identity
        }
    }
}

/// **A strong validator for these bytes**, a representation's own: a
/// compressed form is not the same bytes as the document (RFC 9110 §8.8.1).
pub(crate) fn strong_tag(bytes: &[u8]) -> String {
    let hex: String = sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("\"{hex}\"")
}

/// Brotli at its highest quality, what a static host precompresses with. Two
/// hosts of one build make the same bytes, so a validator one gave holds at
/// the other.
fn brotli_of(text: &[u8]) -> Result<Vec<u8>, String> {
    let params = brotli::enc::BrotliEncoderParams {
        quality: 11,
        size_hint: text.len(),
        ..Default::default()
    };
    let mut out = Vec::new();
    brotli::BrotliCompress(&mut &text[..], &mut out, &params)
        .map_err(|why| format!("Brotli: {why}"))?;
    Ok(out)
}

/// **gzip**, RFC 1952: DEFLATE at the slowest level between the header and
/// the trailer. No time in the header, so the same bytes every time.
fn gzip_of(text: &[u8]) -> Vec<u8> {
    const SLOWEST: u8 = 10;
    // Magic, CM = DEFLATE, no flags, no MTIME, XFL = slowest, OS = unknown.
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 2, 255];
    out.extend(miniz_oxide::deflate::compress_to_vec(text, SLOWEST));
    out.extend(crc32(text).to_le_bytes());
    out.extend((text.len() as u32).to_le_bytes());
    out
}

/// CRC-32 as RFC 1952 computes it, a bit at a time: it runs once a document.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}
