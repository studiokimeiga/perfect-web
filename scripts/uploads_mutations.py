#!/usr/bin/env python3
"""Mutation controls for the uploads track (ADR-0260): an image on a post.

Each mutant undoes one piece, and the tests must then fail:

- what a file is, sniffed from its bytes: the kind taken from the sender, a
  served image typed by its path's extension, a PNG's CRC unread;
- its limits: the size raised before the body is read, and after; its width
  and height unchecked; a JPEG's Exif turn ignored; a deployment raising a
  program's limit;
- where it is kept and served: a key that is not 64 hex digits taken, so a
  path is built from what a sender wrote; `nosniff`, the sandbox or the
  immutable cache dropped; a lease served to another session (one served
  where committed images are, before its post commits, is retired as
  equivalent by ADR-0261: no post names a lease's bytes, and that is asked
  first);
- who: one signed out attaching an image; a user's hourly count, or bytes,
  not held;
- its life: an upload from another origin taken; a claim a command did not
  commit not given back; a discarded lease kept; a post's image committed
  without its bytes in the deployment's storage (in memory and on
  PostgreSQL's layer alike);
- a deleted post's image (ADR-0261): a blob served though no post names
  it, a layer that cannot say taken to say yes, a claim in flight not
  keeping its blob, a blob collected that a post still names or none
  collected, and a layer naming none, in memory and on PostgreSQL's layer;
- the language: PW5603's form posting anywhere, PW5602's route shared with a
  page, PW5601's limit above what a host reads whole;
- the page: an image's width and height left out of its markup, killed by
  the browser suite (`e2e/uploads.spec.mjs`), whose build `feed.sh` makes
  for that mutant and again once it is restored.

The tests: the server's (`uploads` and `blob`, PostgreSQL's where
`PW_FEED_DATABASE_URL` names a database) and the compiler's
(`compiler/pw-core/tests/uploads.rs`). Run from the repository root; `just
e14-uploads` records the output. The source is restored after every mutant,
whatever happens.
"""

import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
UPLOADS = ROOT / "spikes/own-renderer/server/src/uploads.rs"
SNIFF = ROOT / "spikes/own-renderer/server/src/uploads/sniff.rs"
BLOB = ROOT / "spikes/own-renderer/server/src/blob.rs"
FEED = ROOT / "spikes/own-renderer/server/src/feed.rs"
PG = ROOT / "spikes/own-renderer/server/src/feed_pg.rs"
CHECK = ROOT / "compiler/pw-core/src/uploads.rs"
APP = ROOT / "examples/feed/app.pw"

RUST = "rust"
BROWSER = "browser"

# (what is undone, which tests, file, anchor, replacement): the shape
# scripts/mutation_anchors.py reads, its last three the file and the edit.
MUTANTS = [
    (
        "the kind is the sender's: anything is a PNG",
        RUST,
        SNIFF,
        "    let kind = sniff(bytes).ok_or(Unreadable::Unrecognized)?;\n",
        "    let kind = sniff(bytes).unwrap_or(Kind::Png);\n",
    ),
    (
        "a served image is typed by its path's extension",
        RUST,
        UPLOADS,
        "            let kind = sniff::sniff(&bytes)?;\n",
        "            let kind = Kind::named(ext)?;\n",
    ),
    (
        "a PNG's IHDR CRC is not checked",
        RUST,
        SNIFF,
        "    if crc32(&b[12..29]) != crc {\n",
        "    if crc32(&b[12..29]) != crc && false {\n",
    ),
    (
        "the size limit raised before the body is read",
        RUST,
        UPLOADS,
        "        if length > declared.max_bytes + FORM_OVERHEAD {\n",
        "        if length > 1000 * declared.max_bytes + FORM_OVERHEAD {\n",
    ),
    (
        "the size limit raised after the body is read",
        RUST,
        UPLOADS,
        "        if bytes.len() as u64 > declared.max_bytes {\n",
        "        if bytes.len() as u64 > declared.max_bytes + 1 {\n",
    ),
    (
        "the width and height are not checked",
        RUST,
        UPLOADS,
        "        if measured.width > declared.max_width || measured.height > declared.max_height {\n",
        "        if false {\n",
    ),
    (
        "a JPEG's Exif turn is ignored",
        RUST,
        SNIFF,
        "            return Ok(if turned { (y, x) } else { (x, y) });\n",
        "            return Ok((x, y));\n",
    ),
    (
        "a deployment may raise a program's limit",
        RUST,
        UPLOADS,
        "                Some(n) if n > *ours => Err(format!(\n",
        "                Some(n) if n > u64::MAX - 1 => Err(format!(\n",
    ),
    (
        "a key need not be 64 hex digits: a path built from what a sender wrote",
        RUST,
        BLOB,
        "        (text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))\n",
        "        (!text.is_empty())\n",
    ),
    (
        "nosniff dropped",
        RUST,
        UPLOADS,
        '    ("x-content-type-options", "nosniff"),\n',
        "",
    ),
    (
        "the sandbox dropped",
        RUST,
        UPLOADS,
        "    (\"content-security-policy\", \"default-src 'none'; sandbox\"),\n",
        "",
    ),
    (
        "a committed image is not kept by caches as immutable",
        RUST,
        UPLOADS,
        '            "public, max-age=31536000, immutable".to_string(),\n',
        '            "no-cache".to_string(),\n',
    ),
    (
        "a lease is shown to any session",
        RUST,
        UPLOADS,
        "            state\n"
        "                .leases\n"
        "                .get(session)\n"
        "                .filter(|l| l.id == id)\n",
        "            state\n"
        "                .leases\n"
        "                .values()\n"
        "                .find(|l| l.id == id && !session.is_empty())\n",
    ),
    (
        "an upload from another origin is taken",
        RUST,
        UPLOADS,
        "        if !same_origin(headers) {\n",
        "        if !same_origin(headers) && false {\n",
    ),
    (
        "a claim no transaction committed is not given back",
        RUST,
        UPLOADS,
        "        if self.settled {\n            return;\n        }\n",
        "        if self.settled || !self.settled {\n            return;\n        }\n",
    ),
    (
        "a discarded lease is kept",
        RUST,
        UPLOADS,
        "                Fate::Discarded => leases.discarded(claimed),\n",
        "                Fate::Discarded => leases.unclaim(claimed),\n",
    ),
    ("a post's image commits without its bytes in the deployment's storage", RUST, FEED, '    fn prepare_materialized(&mut self) -> Result<(), String> {\n        self.claims.lock().expect("claims").keep()\n    }\n', '    fn prepare_materialized(&mut self) -> Result<(), String> {\n        Ok(())\n    }\n'),
    (
        "on PostgreSQL, a post's image commits without its bytes in storage",
        "postgres",
        PG,
        '        self.claims.lock().expect("claims").keep()?;\n',
        "",
    ),
    (
        "one signed out may attach an image",
        RUST,
        UPLOADS,
        "            Some(principals) => principals.of(session).map(|p| p.user),\n",
        "            Some(principals) => Some(principals.user_of(session)),\n",
    ),
    (
        "a user's hourly count of images is not held",
        RUST,
        UPLOADS,
        "        if spent.len() >= UPLOADS_AN_HOUR || bytes_spent + size > BYTES_AN_HOUR * declared.max_bytes\n",
        "        if bytes_spent + size > BYTES_AN_HOUR * declared.max_bytes\n",
    ),
    (
        "a user's hourly bytes are not held",
        RUST,
        UPLOADS,
        "        if spent.len() >= UPLOADS_AN_HOUR || bytes_spent + size > BYTES_AN_HOUR * declared.max_bytes\n",
        "        if spent.len() >= UPLOADS_AN_HOUR\n",
    ),
    (
        "a blob is served whether or not a post names it",
        RUST,
        UPLOADS,
        "            Some(Ok(false)) | None => return Answer::text(404, \"not found\"),\n",
        "            Some(Ok(false)) | None => {}\n",
    ),
    (
        "a layer that cannot say whether a post names a blob is taken to say yes",
        RUST,
        UPLOADS,
        "                return Answer::text(503, \"unavailable\");\n",
        "",
    ),
    (
        "a claim in flight does not keep its blob",
        RUST,
        UPLOADS,
        "        if named || state.leases.values().any(|l| l.claimed && l.key == key) {\n",
        "        if named {\n",
    ),
    (
        "the memory layer collects a blob a post still names",
        RUST,
        FEED,
        "                let named = names(&self.state.posts, &key);\n",
        "                let named = false;\n",
    ),
    (
        "the memory layer collects no deleted post's image",
        RUST,
        FEED,
        "                if let Err(why) = leases.collect(&key, named) {\n",
        "                if let Err(why) = Ok::<bool, String>(named) {\n",
    ),
    (
        "the memory layer names no blob, so none is served",
        RUST,
        FEED,
        "        Ok(names(&self.state.lock().expect(\"feed\").posts, key))\n",
        "        Ok(false)\n",
    ),
    (
        "PostgreSQL's layer collects no deleted post's image",
        "postgres",
        PG,
        "                    .and_then(|named| leases.collect(&key, named));\n",
        "                    .map(|named| named);\n",
    ),
    (
        "PostgreSQL's layer names no blob, so none is served",
        "postgres",
        PG,
        "        self.pool.with(|c| names_blob(c, key))\n",
        "        self.pool.with(|c| names_blob(c, key)).map(|_| false)\n",
    ),
    (
        "PW5603: a form that sends a file may post anywhere",
        RUST,
        CHECK,
        "            if to_upload.is_none() {\n",
        "            if to_upload.is_none() && false {\n",
    ),
    (
        "PW5602: an upload's route may be a page's",
        RUST,
        CHECK,
        "                    .find(|(r, _)| route_matches(crate::routes::route_parts(r).0, &path));\n",
        "                    .find(|(r, _)| r.is_empty() && route_matches(crate::routes::route_parts(r).0, &path));\n",
    ),
    (
        "PW5601: a limit above what a host reads whole",
        RUST,
        CHECK,
        "        && bytes <= MOST_BYTES\n",
        "        && bytes > 0\n",
    ),
    (
        "an image's width and height are left out of the timeline's markup",
        BROWSER,
        APP,
        "<img class=\"photo\" src={shown_image.src} width={shown_image.width} "
        "height={shown_image.height} alt={shown_image.alt}>",
        "<img class=\"photo\" src={shown_image.src} alt={shown_image.alt}>",
    ),
]

SERVER = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "uploads", "blob"]
COMPILER = ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "uploads"]
FEED_BUILD = ["bash", "spikes/own-renderer/feed.sh"]
BROWSER_SUITE = ["pnpm", "exec", "playwright", "test", "e2e/uploads.spec.mjs", "--reporter=line"]


def database():
    return bool(os.environ.get("PW_FEED_DATABASE_URL"))


def cargo(cmd):
    """(built, passed, failed) of one cargo test command."""
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", r.stdout + r.stderr)
    if not results:
        return False, 0, 0
    return True, sum(int(p) for p, _ in results), sum(int(f) for _, f in results)


def browser():
    """(built, passed, failed) of the browser suite, on a feed built now."""
    b = subprocess.run(FEED_BUILD, cwd=ROOT, capture_output=True, text=True)
    if b.returncode != 0:
        return False, 0, 0
    r = subprocess.run(
        BROWSER_SUITE, cwd=ROOT / "spikes/own-renderer", capture_output=True, text=True
    )
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", r.stdout + r.stderr)
    passed = sum(int(n) for n in re.findall(r"^\s+(\d+) passed", out, re.M))
    failed = sum(int(n) for n in re.findall(r"^\s+(\d+) (?:failed|did not run)", out, re.M))
    return passed + failed > 0, passed, failed


def run(which):
    runs = [cargo(SERVER)]
    if which == RUST:
        runs.append(cargo(COMPILER))
    if which == BROWSER:
        runs.append(browser())
    built = all(b for b, _, _ in runs)
    return built, sum(p for _, p, _ in runs), sum(f for _, _, f in runs)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    for which in (RUST, BROWSER):
        built, passed, failed = run(which)
        print(f"baseline ({which}): {passed} passed, {failed} failed")
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1
    print(
        "postgres: "
        + ("PW_FEED_DATABASE_URL names a database" if database() else "none named")
    )

    survivors, skipped = 0, 0
    for what, which, path, anchor, replacement in MUTANTS:
        if which == "postgres" and not database():
            print(f"{what}: NOT RUN (no database named; the in-memory layer's is above)")
            skipped += 1
            continue
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = run(RUST if which == "postgres" else which)
        finally:
            path.write_text(original)
            if which == BROWSER:
                subprocess.run(FEED_BUILD, cwd=ROOT, capture_output=True, text=True)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}")

    run_ = len(MUTANTS) - skipped
    print(f"{run_ - survivors} of {run_} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
