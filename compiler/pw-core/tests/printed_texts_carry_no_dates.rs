//! **What the compiler prints tells no story of the project's.**
//!
//! A diagnostic's note is read by someone using Pleris, in `pw check`'s
//! output and on pleris.dev, where the guide shows it. It states the rule and
//! its reason. How the compiler came to hold the rule is history, which the
//! code's comments and the ADRs keep. Until 2026-11-07 fifteen notes told it,
//! dated: PW5048's opened with the owner, signed out, pressing Post
//! (2026-10-08), and fourteen more said what had checked "until 2026-09-26".
//!
//! The rule is read off the source, as the compiler's own words are written
//! there: no string literal of the crates whose text it prints (`pw-core`,
//! `pw-cli`, `pw-syntax`), comments aside, holds a date written
//! `YYYY-MM-DD`. A corpus could not hold it: no fixture reaches most of the
//! codes those notes belong to. Two literals may hold one, each named in
//! `ALLOWED` with its reason, and each must still be found, so that neither
//! outlives what it excuses.

use std::path::{Path, PathBuf};

/// The literals that may hold a date: the file, a piece of the literal's
/// text, and why.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "compiler/pw-core/src/codes.rs",
        "Retired 2026-08-06",
        "the record of a retired code: the date its number was retired",
    ),
    (
        "compiler/pw-core/src/wit.rs",
        "ruling, 2026-08-20",
        "the header of the WIT file `pw emit-wit` writes, which cites the ruling it follows",
    ),
];

/// The crates whose text the compiler prints.
const CRATES: &[&str] = &[
    "compiler/pw-core/src",
    "compiler/pw-cli/src",
    "compiler/pw-syntax/src",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let p = e.expect("entry").path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Where a raw string literal starts at `i` (`r"`, `r#"`, `br"`, ..), the
/// number of its `#`s.
fn raw_start(b: &[u8], i: usize) -> Option<usize> {
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let free = i == 0 || !ident(b[i - 1]) || (b[i - 1] == b'b' && (i < 2 || !ident(b[i - 2])));
    if b[i] != b'r' || !free {
        return None;
    }
    let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
    (b.get(i + 1 + hashes) == Some(&b'"')).then_some(hashes)
}

fn find(b: &[u8], from: usize, what: &[u8]) -> Option<usize> {
    (from..=b.len().saturating_sub(what.len())).find(|&k| b[k..].starts_with(what))
}

/// **Each string literal of `src`**, with the line it opens on, as written
/// (escapes left as they are): comments skipped, char literals and
/// lifetimes told apart.
fn literals(src: &str) -> Vec<(usize, String)> {
    let b = src.as_bytes();
    let (mut i, mut line, mut out) = (0, 1, Vec::new());
    while i < b.len() {
        match b[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut depth = 1;
                i += 2;
                while i < b.len() && depth > 0 {
                    if b[i..].starts_with(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if b[i..].starts_with(b"*/") {
                        depth -= 1;
                        i += 2;
                    } else {
                        line += usize::from(b[i] == b'\n');
                        i += 1;
                    }
                }
            }
            b'r' if raw_start(b, i).is_some() => {
                let hashes = raw_start(b, i).expect("raw");
                let open = i + hashes + 2;
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                let end = find(b, open, &close).expect("a raw string closes");
                let text = &src[open..end];
                out.push((line, text.to_string()));
                line += text.matches('\n').count();
                i = end + close.len();
            }
            b'"' => {
                let (start, mut j, mut text) = (line, i + 1, Vec::new());
                while b[j] != b'"' {
                    let n = if b[j] == b'\\' { 2 } else { 1 };
                    line += b[j..j + n].iter().filter(|&&c| c == b'\n').count();
                    text.extend_from_slice(&b[j..j + n]);
                    j += n;
                }
                out.push((start, String::from_utf8_lossy(&text).into_owned()));
                i = j + 1;
            }
            b'\'' => {
                // A char literal ('x', '\n', '\'', '…') or a lifetime ('a).
                // An escape's closing quote is after the escaped character.
                let mut chars = src[i + 1..].chars();
                i += match (chars.next(), chars.next()) {
                    (Some('\\'), _) => find(b, i + 3, b"'").expect("a char closes") + 1 - i,
                    (Some(c), Some('\'')) => c.len_utf8() + 2,
                    _ => 1,
                };
            }
            _ => i += 1,
        }
    }
    out
}

/// The first date written `YYYY-MM-DD` in `text`, digits not running on
/// either side.
fn date_in(text: &str) -> Option<&str> {
    let b = text.as_bytes();
    let shape = |w: &[u8]| {
        w.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
    };
    (0..b.len().saturating_sub(9)).find_map(|k| {
        let alone = (k == 0 || !b[k - 1].is_ascii_digit())
            && b.get(k + 10).is_none_or(|c| !c.is_ascii_digit());
        (alone && shape(&b[k..k + 10])).then(|| &text[k..k + 10])
    })
}

#[test]
fn no_string_the_compiler_prints_holds_a_date() {
    let root = repo();
    let mut files = Vec::new();
    for c in CRATES {
        let before = files.len();
        rust_files(&root.join(c), &mut files);
        assert!(files.len() > before, "{c}: no Rust file read");
    }
    files.sort();
    let mut found = Vec::new();
    let mut allowed_seen = vec![0; ALLOWED.len()];
    for f in &files {
        let rel = f
            .strip_prefix(&root)
            .expect("in the repository")
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(f).expect("read");
        for (line, text) in literals(&src) {
            let Some(date) = date_in(&text) else { continue };
            match ALLOWED
                .iter()
                .position(|(file, piece, _)| *file == rel && text.contains(piece))
            {
                Some(k) => allowed_seen[k] += 1,
                None => found.push(format!("{rel}:{line}: {date} in {text:?}")),
            }
        }
    }
    assert!(
        found.is_empty(),
        "say the rule and its reason, and keep the history in a comment or an ADR:\n{}",
        found.join("\n")
    );
    for ((file, piece, why), seen) in ALLOWED.iter().zip(&allowed_seen) {
        assert_eq!(
            *seen, 1,
            "{file}: the allowed literal holding `{piece}` ({why}) is found {seen} times"
        );
    }
}

/// The reader itself: what it finds, and what it passes over.
#[test]
fn a_literal_is_read_as_rust_reads_it() {
    let src = concat!(
        "// a comment, 2026-01-01\n",
        "/// a doc comment, 2026-01-02\n",
        "/* a block /* nested, 2026-01-03 */ still */\n",
        "fn f<'a>(x: &'a str) -> char {\n",
        "    let q = '\"'; let e = '\\''; let u = '…';\n",
        "    let s = \"until \\\n",
        "             2026-01-04, and \\\"quoted\\\" 2026-01-05\";\n",
        "    let r = r#\"raw \"2026-01-06\" here\"#;\n",
        "    let b = b\"bytes\"; let n = \"20260107\";\n",
        "    'x'\n",
        "}\n",
    );
    let read = literals(src);
    let dated: Vec<(usize, &str)> = read
        .iter()
        .filter_map(|(line, t)| date_in(t).map(|d| (*line, d)))
        .collect();
    assert_eq!(dated, [(6, "2026-01-04"), (8, "2026-01-06")], "{read:#?}");
    // Every literal, and none of the comments or char literals.
    assert_eq!(read.len(), 4, "{read:#?}");
    assert!(
        read[0].1.contains("2026-01-05"),
        "the escaped quote did not end it"
    );
    assert_eq!(read[2].1, "bytes");
    assert_eq!(date_in(&read[3].1), None, "eight digits are no date");
}
