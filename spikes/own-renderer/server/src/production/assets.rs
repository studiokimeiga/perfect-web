//! **A production host's build assets** (track `deploy`): the runtime, its
//! WebAssembly, the build's stylesheets and drawings, and the modules of its
//! handlers, speculations and computed values, read once at start and
//! served from memory.
//!
//! - **Validated by their digest.** Each answer carries a strong `ETag`, the
//!   SHA-256 of what it holds, and a request that already holds it is
//!   answered `304` with nothing (RFC 9110 §13.1.2, §15.4.5).
//! - **Kept for good at an address that changes with them.** The build's
//!   version is a digest of every asset; the runtime is started from
//!   `/pw-runtime.mjs?v=<version>`, and loads its WebAssembly and its
//!   handlers with the same `v` (`asset` in `pw-runtime.mjs`). An answer to
//!   an address with this build's `v` is `immutable` for a year (RFC 8246);
//!   any other is `no-cache`, kept but asked again, so a page of an older
//!   build that asks with its own `v` is never given another build's bytes
//!   to keep under its address.
//! - **Compressed once.** Each is gzipped at start (RFC 1952: miniz_oxide's
//!   DEFLATE at its best level, crc32fast's CRC-32) where that is smaller,
//!   and sent so where the request accepts it, with `Vary:
//!   Accept-Encoding`. Brotli is not here: no crate in Cargo.lock writes it
//!   (the ADR's gap). A document is never compressed: what a session's
//!   answer holds is its reader's, beside what its address reflects, which
//!   is BREACH's precondition.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::identity::header;

/// What a cache may do with an answer at this build's version: keep it for
/// a year without asking again (RFC 8246 `immutable`).
pub(crate) const KEPT: &str = "public, max-age=31536000, immutable";
/// What a cache may do with any other: keep it, and ask before using it.
pub(crate) const ASKED: &str = "no-cache";

/// The extensions a build's root holds that a page loads: its runtime and
/// modules, its WebAssembly, its stylesheets, its drawings. Never a `.html`
/// (a page is the build's route, not a file) or a `.json` (`sources.json`
/// names the paths of the machine that built it).
pub(crate) const SERVED: &[&str] = &["mjs", "js", "wasm", "css", "svg"];

/// A file's type, by its extension, as `serve_file` gives it.
pub(crate) fn mime_of(route: &str) -> &'static str {
    match route.rsplit_once('.').map(|(_, e)| e) {
        Some("mjs") | Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// **One asset**: its bytes, its gzip where that is smaller, its digest.
struct Asset {
    mime: &'static str,
    bytes: Vec<u8>,
    gzip: Option<Vec<u8>>,
    /// The first 32 hexadecimal digits of the bytes' SHA-256.
    digest: String,
}

/// **The build's assets, by the route each is asked by**, and the build's
/// version.
pub(crate) struct Assets {
    files: BTreeMap<String, Asset>,
    version: String,
}

/// **An answer from the table**: its status, type, headers (each line ending
/// in CRLF) and body.
#[derive(Debug)]
pub(crate) struct Answer<'a> {
    pub(crate) status: u16,
    pub(crate) mime: &'static str,
    pub(crate) headers: String,
    pub(crate) body: &'a [u8],
}

impl Assets {
    /// **Each `(route, file)` read**, or why one could not be.
    pub(crate) fn read(files: Vec<(String, std::path::PathBuf)>) -> Result<Assets, String> {
        let mut read = Vec::new();
        for (route, path) in files {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            read.push((route, bytes));
        }
        Ok(Assets::of(read))
    }

    /// **The table of `(route, bytes)`**: each digested and compressed, and
    /// the version their digests make.
    pub(crate) fn of(files: Vec<(String, Vec<u8>)>) -> Assets {
        let mut table = BTreeMap::new();
        for (route, bytes) in files {
            let digest = hex(&Sha256::digest(&bytes))[..32].to_string();
            let gzip = Some(gzip(&bytes)).filter(|g| g.len() < bytes.len());
            table.insert(
                route.clone(),
                Asset {
                    mime: mime_of(&route),
                    bytes,
                    gzip,
                    digest,
                },
            );
        }
        // Every route and its digest, in order: any asset changed, added or
        // taken away is another version.
        let mut all = Sha256::new();
        for (route, asset) in &table {
            all.update(route.as_bytes());
            all.update(b"\0");
            all.update(asset.digest.as_bytes());
            all.update(b"\n");
        }
        Assets {
            version: hex(&all.finalize())[..16].to_string(),
            files: table,
        }
    }

    /// **The build's version**: what `v` says in an address this build gives.
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// The routes the table holds.
    pub(crate) fn routes(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// **The answer to a `GET` of `route`**, or none where it is no asset.
    pub(crate) fn answer(&self, route: &str, query: &str, headers: &str) -> Option<Answer<'_>> {
        let asset = self.files.get(route)?;
        let versioned = query
            .split('&')
            .any(|p| p.strip_prefix("v=") == Some(self.version.as_str()));
        let cache = if versioned { KEPT } else { ASKED };
        let gzipped = asset.gzip.as_ref().filter(|_| accepts_gzip(headers));
        let etag = match gzipped {
            // Another representation is another strong validator (RFC 9110
            // §8.8.3): the same digest, marked.
            Some(_) => format!("\"{}-gz\"", asset.digest),
            None => format!("\"{}\"", asset.digest),
        };
        let vary = if asset.gzip.is_some() {
            "vary: accept-encoding\r\n"
        } else {
            ""
        };
        let mut lines = format!("cache-control: {cache}\r\netag: {etag}\r\n{vary}");
        if holds(headers, &asset.digest) {
            return Some(Answer {
                status: 304,
                mime: asset.mime,
                headers: lines,
                body: &[],
            });
        }
        let body = match gzipped {
            Some(gzip) => {
                lines.push_str("content-encoding: gzip\r\n");
                gzip.as_slice()
            }
            None => asset.bytes.as_slice(),
        };
        Some(Answer {
            status: 200,
            mime: asset.mime,
            headers: lines,
            body,
        })
    }
}

/// **Whether the request already holds the asset whose digest is
/// `digest`**: `If-None-Match` lists its tag, in either representation, or
/// is `*` (RFC 9110 §13.1.2, compared weakly as it says).
fn holds(headers: &str, digest: &str) -> bool {
    let Some(tags) = header(headers, "if-none-match") else {
        return false;
    };
    tags.split(',').map(str::trim).any(|tag| {
        let tag = tag.strip_prefix("W/").unwrap_or(tag);
        tag == "*" || tag == format!("\"{digest}\"") || tag == format!("\"{digest}-gz\"")
    })
}

/// **Whether the request accepts gzip** (RFC 9110 §12.5.3): `gzip` or
/// `x-gzip` listed with a weight above zero, or `*` so where gzip is not
/// listed.
pub(crate) fn accepts_gzip(headers: &str) -> bool {
    let Some(value) = header(headers, "accept-encoding") else {
        return false;
    };
    let (mut gzip, mut any) = (None, None);
    for item in value.split(',') {
        let mut parts = item.split(';');
        let coding = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        let weight = parts
            .find_map(|p| {
                let (k, v) = p.split_once('=')?;
                k.trim()
                    .eq_ignore_ascii_case("q")
                    .then(|| v.trim().parse::<f32>().ok())?
            })
            .unwrap_or(1.0);
        match coding.as_str() {
            "gzip" | "x-gzip" => gzip = Some(weight),
            "*" => any = Some(weight),
            _ => {}
        }
    }
    match gzip {
        Some(weight) => weight > 0.0,
        None => any.is_some_and(|weight| weight > 0.0),
    }
}

/// **`bytes` as one gzip member** (RFC 1952 §2.3): its fixed header (no
/// name, no time, so the same bytes make the same member), the DEFLATE
/// stream at miniz_oxide's best standard level, and the CRC-32 and the
/// length of what it holds.
pub(crate) fn gzip(bytes: &[u8]) -> Vec<u8> {
    // ID1 ID2, CM = 8 (DEFLATE), FLG = 0, MTIME = 0, XFL = 2 (the slowest,
    // best compression), OS = 255 (unknown).
    let mut member = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 2, 255];
    member.extend(miniz_oxide::deflate::compress_to_vec(bytes, 9));
    member.extend(crc32fast::hash(bytes).to_le_bytes());
    // ISIZE: the length modulo 2^32, as the format says.
    member.extend((bytes.len() as u32).to_le_bytes());
    member
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table of three assets: the runtime, a WebAssembly module, and a
    /// drawing too small to be worth compressing.
    fn table() -> Assets {
        let runtime = "// the runtime\n".repeat(400).into_bytes();
        let wasm = [b"\0asm\x01\0\0\0".as_slice(), &[0u8; 4000]].concat();
        Assets::of(vec![
            ("/pw-runtime.mjs".to_string(), runtime),
            ("/pw-render.wasm".to_string(), wasm),
            ("/check.svg".to_string(), b"<svg/>".to_vec()),
        ])
    }

    /// The member's DEFLATE stream, decompressed, after its header is
    /// checked; and its trailer checked against what it holds.
    fn gunzip(member: &[u8]) -> Vec<u8> {
        assert_eq!(&member[..10], &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 2, 255]);
        let (body, trailer) = member[10..].split_at(member.len() - 18);
        let held = miniz_oxide::inflate::decompress_to_vec(body).expect("DEFLATE");
        assert_eq!(&trailer[..4], &crc32fast::hash(&held).to_le_bytes());
        assert_eq!(&trailer[4..], &(held.len() as u32).to_le_bytes());
        held
    }

    fn head(headers: &str, name: &str) -> Option<String> {
        header(headers, name).map(str::to_string)
    }

    /// **A gzip member holds what it compressed**: its header is RFC
    /// 1952's, its trailer the CRC-32 and the length of the bytes, and the
    /// same bytes make the same member.
    #[test]
    fn a_gzip_member_holds_what_it_compressed() {
        for bytes in [
            Vec::new(),
            b"a".to_vec(),
            "pleris ".repeat(5000).into_bytes(),
        ] {
            let member = gzip(&bytes);
            assert_eq!(gunzip(&member), bytes);
            assert_eq!(gzip(&bytes), member);
        }
        // Known CRC-32 (RFC 1952's, as zlib computes it): "123456789".
        assert_eq!(
            &gzip(b"123456789")[gzip(b"123456789").len() - 8..][..4],
            &0xcbf4_3926u32.to_le_bytes()
        );
    }

    /// **An asset is sent compressed where the request accepts gzip**, with
    /// `Vary`, and as it is otherwise; one too small to gain is never.
    #[test]
    fn an_asset_is_compressed_where_the_request_accepts_it() {
        let assets = table();
        let plain = assets
            .answer("/pw-runtime.mjs", "", "Host: t\r\n")
            .expect("an asset");
        assert_eq!(plain.status, 200);
        assert_eq!(head(&plain.headers, "content-encoding"), None);
        assert_eq!(
            head(&plain.headers, "vary").as_deref(),
            Some("accept-encoding")
        );
        let original = plain.body.to_vec();

        let gzipped = assets
            .answer(
                "/pw-runtime.mjs",
                "",
                "Accept-Encoding: gzip, deflate, br\r\n",
            )
            .expect("an asset");
        assert_eq!(
            head(&gzipped.headers, "content-encoding").as_deref(),
            Some("gzip")
        );
        assert!(
            gzipped.body.len() * 10 < original.len(),
            "{}",
            gzipped.body.len()
        );
        assert_eq!(gunzip(gzipped.body), original);
        assert_eq!(gzipped.mime, "text/javascript; charset=utf-8");

        let small = assets
            .answer("/check.svg", "", "Accept-Encoding: gzip\r\n")
            .expect("an asset");
        assert_eq!(small.body, b"<svg/>");
        assert_eq!(head(&small.headers, "content-encoding"), None);
        assert_eq!(head(&small.headers, "vary"), None);
        assert_eq!(small.mime, "image/svg+xml");
    }

    /// **Who accepts gzip** (RFC 9110 §12.5.3): listed, in any case, at a
    /// weight above zero; or `*` where gzip is not listed.
    #[test]
    fn who_accepts_gzip() {
        let accepts = |v: &str| accepts_gzip(&format!("Accept-Encoding: {v}\r\n"));
        assert!(accepts("gzip"));
        assert!(accepts("GZip;q=0.5"));
        assert!(accepts("br, x-gzip"));
        assert!(accepts("*"));
        assert!(accepts("br;q=1.0, gzip;q=0.001"));
        assert!(!accepts("gzip;q=0"));
        assert!(!accepts("gzip;q=0.000"));
        assert!(!accepts("*, gzip;q=0"));
        assert!(!accepts("*;q=0"));
        assert!(!accepts("br, deflate"));
        assert!(!accepts("identity"));
        assert!(!accepts_gzip("Host: t\r\n"));
    }

    /// **An address at this build's version is kept for good; any other is
    /// asked again**, an older build's version among them.
    #[test]
    fn an_address_at_this_builds_version_is_kept_for_good() {
        let assets = table();
        let at = |query: &str| {
            let a = assets
                .answer("/pw-render.wasm", query, "")
                .expect("an asset");
            head(&a.headers, "cache-control").expect("a cache-control")
        };
        assert_eq!(at(&format!("v={}", assets.version())), KEPT);
        assert_eq!(at(&format!("attempt=1&v={}", assets.version())), KEPT);
        assert_eq!(at(""), ASKED);
        assert_eq!(at("v=0000000000000000"), ASKED);
        assert_eq!(at(&format!("v={}0", assets.version())), ASKED);
        assert_eq!(at(&format!("xv={}", assets.version())), ASKED);
    }

    /// **A request that holds the asset is answered `304`, with nothing**:
    /// by either representation's tag, weak or strong, or `*`; a tag of
    /// other bytes is answered whole.
    #[test]
    fn a_request_that_holds_the_asset_is_answered_with_nothing() {
        let assets = table();
        let first = assets
            .answer("/pw-runtime.mjs", "", "Accept-Encoding: gzip\r\n")
            .expect("an asset");
        let tag = head(&first.headers, "etag").expect("an etag");
        assert!(tag.ends_with("-gz\""), "{tag}");
        let plain_tag = head(
            &assets
                .answer("/pw-runtime.mjs", "", "")
                .expect("an asset")
                .headers,
            "etag",
        )
        .expect("an etag");
        assert_ne!(tag, plain_tag);
        for held in [
            tag.clone(),
            plain_tag.clone(),
            format!("W/{tag}"),
            "*".into(),
            format!("\"x\", {plain_tag}"),
        ] {
            let again = assets
                .answer(
                    "/pw-runtime.mjs",
                    "",
                    &format!("If-None-Match: {held}\r\nAccept-Encoding: gzip\r\n"),
                )
                .expect("an asset");
            assert_eq!(again.status, 304, "{held}");
            assert!(again.body.is_empty());
            assert_eq!(head(&again.headers, "etag"), Some(tag.clone()), "{held}");
            assert_eq!(
                head(&again.headers, "cache-control").as_deref(),
                Some(ASKED)
            );
        }
        let other = assets
            .answer(
                "/pw-runtime.mjs",
                "",
                "If-None-Match: \"0123456789abcdef0123456789abcdef\"\r\n",
            )
            .expect("an asset");
        assert_eq!(other.status, 200);
    }

    /// **The version is every asset's**: one byte of one changed, one added
    /// or one taken away is another version; the same assets, the same.
    #[test]
    fn the_version_is_every_assets() {
        let version = |files: Vec<(&str, &[u8])>| {
            Assets::of(
                files
                    .into_iter()
                    .map(|(r, b)| (r.to_string(), b.to_vec()))
                    .collect(),
            )
            .version()
            .to_string()
        };
        let base = version(vec![("/a.mjs", b"a"), ("/b.wasm", b"b")]);
        assert_eq!(base.len(), 16);
        assert_eq!(base, version(vec![("/b.wasm", b"b"), ("/a.mjs", b"a")]));
        assert_ne!(base, version(vec![("/a.mjs", b"a"), ("/b.wasm", b"c")]));
        assert_ne!(base, version(vec![("/a.mjs", b"a")]));
        assert_ne!(
            base,
            version(vec![("/a.mjs", b"a"), ("/b.wasm", b"b"), ("/c.css", b"")])
        );
        assert_ne!(base, version(vec![("/a.mjs", b"a"), ("/c.wasm", b"b")]));
    }

    /// **Only a route the table holds is answered**; a type is its
    /// extension's.
    #[test]
    fn only_a_route_the_table_holds_is_answered() {
        let assets = table();
        assert!(assets.answer("/sources.json", "", "").is_none());
        assert!(assets.answer("/pw-runtime.mjs/", "", "").is_none());
        assert!(assets.answer("/PW-RUNTIME.MJS", "", "").is_none());
        assert_eq!(mime_of("/x.wasm"), "application/wasm");
        assert_eq!(mime_of("/x.css"), "text/css; charset=utf-8");
        assert_eq!(mime_of("/x.js"), "text/javascript; charset=utf-8");
        assert_eq!(mime_of("/x"), "application/octet-stream");
        assert_eq!(
            assets.routes().collect::<Vec<_>>(),
            vec!["/check.svg", "/pw-render.wasm", "/pw-runtime.mjs"]
        );
    }
}
