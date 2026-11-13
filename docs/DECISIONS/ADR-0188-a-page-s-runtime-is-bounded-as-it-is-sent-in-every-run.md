# ADR-0188: a page's runtime is bounded as it is sent, in every run

Status: accepted under the owner's delegation of 2026-10-02. Date: 2026-10-04.
Amended 2026-11-13 by ADR-0439: the interactive route's activation is bounded
at 72 KiB.
Milestone: E14, with E7's gate item 7. A correction to E7's record, found
while measuring ADR-0187.

## Context

- **E7's gate item 7** (charter §14 M7): "Browser runtime size and
  activation CPU are measured."
  - **Its test bounded the bytes** the store's page downloads to run: the
    runtime's script and the resume's WebAssembly, at 128 KiB. It said of
    itself: "A bound, not a target. It exists so the number cannot quietly
    become a megabyte."
  - **At E7, on 2026-08-07, they were 102,693 bytes**: 28,471 of script and
    74,222 of WebAssembly.
- **It ran only in `just e7-performance`.** That is excluded from the browser
  suite, because the long animation frames it measures need the machine to
  themselves. It had last run on 2026-08-07.
- **On 2026-10-04 they were 141,339 bytes**: 84,822 of script and 56,517 of
  WebAssembly. Gate item 7b had failed since the script grew past the bound,
  between ADR-0152 (2026-10-03, 60,637 bytes of script) and ADR-0172
  (2026-10-04, 82,047). Nothing said so, because nothing ran it.
- **What the bound measured.** The bytes the development server sends:
  uncompressed, as written.
  - **About 45% of the script's bytes are comments and blank lines.**
    Without them it is 46,621 bytes.
  - **A deployment sends a static file compressed.** The charter states a
    change's client impact in compressed bytes (§19's report:
    "+1.8 KB compressed").
  - **Compressed with Brotli at quality 11**, the script is 23,244 bytes
    and the resume's WebAssembly 20,885: 44,129 together. With gzip at
    level 9 they are 27,133 and 24,439.
- **The renderer's WebAssembly** is fetched the first time a block a signal
  decides is rendered again (ADR-0137). It is 343,721 bytes, 82,888 with
  Brotli, and no bound held it. It is already built for size
  (`opt-level = "z"`, LTO, abort on panic, stripped).

## Decision

1. **A page's runtime is bounded as it is sent**: each file's bytes
   compressed with Brotli at quality 11, computed from what the page
   downloads. The uncompressed and gzip figures are reported beside it, and
   E10's record reads the uncompressed one as before.
   - **The interactive route's activation**, the script and the resume's
     WebAssembly: 64 KiB. It is 44,129 bytes today.
   - **The renderer's WebAssembly**: 128 KiB. It is 82,888 bytes today.
   - **The static route**: nothing, as before.
2. **The byte counts run in every browser suite**, in Chromium, from
   `e2e/runtime-size.spec.mjs`. A byte count does not need the machine to
   itself. `just e7-performance` runs that file too, so E7's record keeps
   its sizes.
3. **E7's record is recorded again**, for the first time since 2026-08-07,
   with its gate items 7 to 10.
4. **Controls.** The script grown by 48 KB that does not compress fails gate
   item 7b. The renderer grown by 64 KB that does not compress fails 7d.

## For the owner

**Minifying the runtime.** A minifier would remove the comments and shorten
the names, at build, in `run.sh`. The browser suite would then run against
the minified script. That would cut the script's bytes by about half or more
as sent. It needs a minifier, such as esbuild, which this session did not
download, because a download is the owner's to allow. ADR-0182's axe-core is
the same.

## Alternatives

- **Raise the uncompressed bound**, to 192 KiB. The same blind spot, with a
  higher number, and a bound on comments.
- **Strip comments with a script of this repository's own.** Only a
  JavaScript tokenizer can tell a comment from a string, a template or a
  regular expression's literal, and a mistake would break the page where no
  test looks.
- **Compress in the development server**, and measure what it sends. The
  development server is not a deployment, and the bound would measure its
  compressor's setting.
- **Bound gzip instead of Brotli.** Every current browser accepts Brotli
  over HTTPS, and a static host sends it. Gzip is reported beside it.
- **Leave the byte counts in the performance run.** That is how the bound
  was passed unseen.

## Acceptance

Recorded by `just e7-performance` in `docs/evidence/E7/performance.txt`
and by `just e14-runtime-size` in `docs/evidence/E14/runtime-size.txt`:

- **`e2e/runtime-size.spec.mjs`**, in the browser suite: gate items 7a, 7b
  and 7d.
- **E7's gate items 7 to 10**, alone in Chromium.
- **`scripts/runtime_size_mutations.py`**: the two controls.

## Not claimed

- **What a person's connection does.** The bound is on bytes, not on time.
- **The runtime minified.** It is the owner's decision, above.
