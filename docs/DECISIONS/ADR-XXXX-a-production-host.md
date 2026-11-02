# ADR-XXXX: a production host refuses what production may not run, and is packaged to run

Status: proposed by track `deploy` (2026-11-02) under the owner's words of
2026-11-02 ("You can deploy. You can handle credentials.") and the
coordinator's brief; the integrator numbers it and lands it by pull
request after an independent review. Milestone: E14, the production path
(NEXT: "a deployment, a container image or a host target"). Builds on
ADR-0285 (kiokun.com is the owner's production target, served first on a
preview origin, no cutover without the owner's go), ADR-0311 (`PW_ORIGIN`),
ADR-0333 (what a page may run), ADR-0258 and ADR-0389 (identity and session
authority), ADR-0369 (sessions kept where the hosts share them) and
ADR-0387 (outbound HTTPS, and its `Secret`).

## Context

- **The host is the development server** ("NOT E8's production host" in its
  own Cargo.toml; capabilities.md's Production row: "Pieces, not a
  deployment"). `PW_DEPLOYMENT=production` and `PW_ORIGIN` existed, a
  generated policy, migrations at start under an advisory lock, `/metrics`.
- **What was read before this track** (2026-11-02, master 27c1ffba):
  - A production deployment **could not start at all**: both identity models
    (guest, dev-accounts) refuse anything but a development deployment on
    this machine, and no other existed.
  - Its addresses were keyed by constants this public repository holds: the
    resource entries' PRF key (`pw_resource::DEVELOPMENT_KEY`, whose own
    comment says "a deployment mode that forbids it is E8's to add") and
    the instance tokens' (`own-renderer-spike-key`).
  - Test controls answered anyone: `/bench/*` (the clock moved, failures
    armed), E7-P's structural commands, `/menu-entry`, and
    `GET /StorePage.html?items=N`, which rewrites the store's menu in its
    database. `/metrics` answered anyone.
  - `serve_file` served any file of the build's root: `sources.json` names the
    absolute paths of the machine that built it.
  - Build assets were read from disk on each request, uncompressed, with no
    cache headers; the runtime as written (158,077 bytes, comments and all).
  - It bound 127.0.0.1 alone, read a request's head without bound, waited
    for a read without end, and stopped only when killed.
  - Logs were lines of text on two streams.
  - kiokun's sample data, search index and search cache defaulted to
    compile-time `CARGO_MANIFEST_DIR` paths, which do not exist where an
    image runs.
- **No deploy tooling or cloud credential exists on the development PC**
  (no Docker, Podman, Caddy, flyctl, ...).

## Decision

1. **`PW_DEPLOYMENT=production` refuses at start, every reason said at
   once** (`production.rs`, `Settings::read`, `program_refusals`): an origin
   that is not `https://`; no `PW_IDENTITY_KEY`, or one under 64 hexadecimal
   digits; a test's setting (`PW_FEED_TRANSACTIONS=connection`); a store or a
   feed with no database (its data would be the process's memory), a feed's
   uploads with no `PW_BLOB_DIR`, kiokun with no `KIOKUN_DATA` or no
   `PW_CACHE_DIR`; a `PW_BIND`, `PW_SHUTDOWN_GRACE` or `PW_READ_TIMEOUT` it
   cannot read. The development identity models and payment provider are
   refused where they are configured, as before.
2. **The one identity production can run today is `PW_IDENTITY=none`**: no
   sign-in, every session anonymous (`requires SignedIn` refuses), and
   `/sign-in`, `/sign-up` and the callback answered 404 with a page that
   says so. It runs anywhere, since it signs no one in. A guest's cart is a
   session's, as ADR-0318 has it.
3. **Its addresses are keyed by the deployment's secret**: `PW_IDENTITY_KEY`
   (32 bytes or more, hexadecimal) split by BLAKE3's `derive_key`, one
   context for resource entries and one for instance tokens; the development
   keys remain where no key is stated. `main.rs`'s `IDENTITY` is
   `production::HostKey`.
4. **Its secrets are read from the environment or a secrets file**
   (`production/secrets.rs`): the names are the host's
   (`PW_IDENTITY_KEY`, `PW_STORE_DATABASE_URL`, `PW_FEED_DATABASE_URL`,
   `PW_METRICS_TOKEN`); the file is `PW_SECRETS_FILE`'s, or systemd's
   credential `secrets` in `$CREDENTIALS_DIRECTORY` (`LoadCredential=`;
   systemd.exec(5): environment variables "are not suitable for passing
   secrets"); a file anyone may read, or its group may write, is refused
   unread (its group may read: systemd shows its ACL'd 0400 credential as
   0440, `src/core/exec-credential.c` of v255); a secret given twice is
   refused; no refusal quotes a line of the file; `Secret` has no
   `Display` and a redacted `Debug`. A database's address is a secret (it may
   hold a password). ADR-0387's `Secret` cannot be used: its value is
   readable only inside `pw-outbound`.
5. **It answers no test control** (`is_test_control`): anything under
   `/bench`, E7-P's commands, `/menu-entry`, `/StorePage.html?items=`; 404, as
   routes it does not have. `/metrics` answers only a bearer of
   `PW_METRICS_TOKEN` (compared in constant time); without one, no one.
6. **It serves its build's assets from memory** (`production/assets.rs`):
   the build root's `.mjs`, `.js`, `.wasm`, `.css`, `.svg` (never a page or a
   `.json`), each handler's, speculation's and computed module, exactly the
   set the routes serve; a build missing one is refused at start. Each has a
   strong `ETag` (its SHA-256; another for its gzip, RFC 9110 §8.8.3) and
   `If-None-Match` is answered 304. **Hashed addresses**: the build's version
   is a digest of every asset; documents start the runtime at
   `/pw-runtime.mjs?v=<version>` (the bootstrap and its policy hash computed
   at start), and the runtime carries the same `v` to its WebAssembly, its
   handlers and its own re-import (`asset()` in `pw-runtime.mjs`; a
   development server gives no `v`, so every address is as it was). An
   answer at this build's `v` is `public, max-age=31536000, immutable`
   (RFC 8246); any other `no-cache`, so a page of an older build never pins
   another build's bytes under its address.
7. **Compression: gzip, assets only.** Each asset is gzipped once at start
   (miniz_oxide's DEFLATE at level 9, crc32fast's CRC-32: both already in
   Cargo.lock) where that is smaller, sent with `Vary: Accept-Encoding` to a
   request that accepts it (RFC 9110 §12.5.3's weights). **A session's answer
   is never compressed**: it holds its reader's data beside what its address
   reflects (a search's query), which is BREACH's precondition; documents are
   3-11 KB, the assets ~600 KB. Brotli is not here: no crate in Cargo.lock
   writes it (the gap: ~11% more on the runtime).
8. **The runtime is minified** by esbuild 0.28.2, which the lockfile already
   held (now declared by `spikes/own-renderer`): 158,077 to 50,078 bytes, and
   gzipped 49,611 to 17,610 (−64%). `deploy/build-dist.sh` builds one
   program's production `dist`; the image installs the same esbuild with
   `npm ci` from `deploy/minify/package-lock.json`, written from
   pnpm-lock.yaml's SHA-512s by `scripts/deploy_minify_lock.py`.
9. **It says whether it lives and is ready**: `GET /_pw/health` (always,
   `no-store`) and `GET /_pw/ready` (503 while it stops, or where its
   database does not answer `SELECT 1` within two seconds to connect), under
   `/_pw/`, which no program route can claim. Neither reads a session.
10. **Every answer carries `Strict-Transport-Security: max-age=31536000`**
    (not `includeSubDomains`, which would bind the origin's other hosts);
    the stream's head now carries `nosniff` too (ADR-0333's ruling 4, missed).
11. **TLS is the proxy's or the platform's, not the host's.** On a VM, Caddy
    (`deploy/vm/Caddyfile`): automatic certificates and renewal, OCSP, HTTP/2
    and HTTP/3 to browsers (the host closes every connection after one
    answer, HTTP/1.1, so a browser's six-connections-per-origin limit would
    meet its long polls without a multiplexing front), a restart waited for
    (`lb_try_duration`); on a platform, its edge. In the host, rustls (in
    Cargo.lock since ADR-0387) would serve TLS but not obtain or renew a
    certificate (an ACME client is a download), nor speak HTTP/2. Caddy has
    no `encode` here (decision 7).
12. **JSON lines**: in production every line the host writes is one JSON
    object on standard error (`ts` RFC 3339 UTC, `level`, `event`, `msg`),
    its start, its refusals, the other modules' error lines, its panics, its
    trace, and one per request (method, route without its query, status,
    milliseconds; a subscription's and a health check's only where they
    failed). Development's lines are unchanged.
13. **It stops when told**: SIGTERM or SIGINT, blocked in every thread before
    any starts and taken by `sigwait` in one, stops the accepting (a
    connection of its own wakes `accept`) and closes its listener at once, so
    a connection made then is refused, which Caddy retries until the next
    process listens (`lb_try_duration`), not queued for a host that will not
    answer it; then it waits for the open connections up to
    `PW_SHUTDOWN_GRACE` (8 s, under Docker's 10) and exits 0. A command cut by
    the grace is one transaction rolled back. A connection that asks nothing
    (a platform's TCP check, the stop's own wake) is answered nothing.
14. **Its connections are bounded**: a request's head is at most 64 KiB (431
    past it, in development too), and in production each read waits at most
    `PW_READ_TIMEOUT` (30 s); it listens where `PW_BIND` says (a development
    server on its loopback alone).
15. **The deployable artifacts** (`deploy/`): `Containerfile` (node stage for
    esbuild; rust:1.97.1-bookworm stage for the host, `pw`, the WebAssembly
    and `build-dist.sh`; distroless `cc-debian12:nonroot`, read-only, no
    shell; each base pinned by its index digest, read from its registry on
    2026-11-02: rust `sha256:0e2bcaef…4a97`, node 22.21.1
    `sha256:25b3eb23…ae2c`, distroless `sha256:9dac0a79…182f`), a root
    `.dockerignore`; and the single-VM recipe `vm/`: `setup.sh` (PostgreSQL 18
    from PGDG with peer authentication, no password; Caddy from its stable
    repository; the `pleris` user; settings; secrets made once on the VM;
    unattended upgrades; a firewall), `pleris.service` (`LoadCredential`,
    `ProtectSystem=strict` and the rest of systemd's confinement; not
    `MemoryDenyWriteExecute`, which Cranelift's compilation at start needs),
    `Caddyfile`, and `release.sh` (a clean commit built here, copied,
    switched, checked at `/_pw/ready`, put back if not ready). `just
    deploy-dist|deploy-image|deploy-smoke|deploy-vm`. Nothing reaches a
    machine without the owner's `~/.config/pleris/deploy.env`.

## Alternatives

- **TLS in the host (rustls)**: no certificate issuance or renewal without
  an ACME crate, no HTTP/2; rejected for the proxy (decision 11).
- **Compressing documents too** (or Caddy's `encode`): BREACH; rejected.
- **ETag and `no-cache` alone, no versioned address**: correct, but a round
  trip per asset per page; the version costs one helper in the runtime.
- **Rewriting asset names at build time** (`pw-runtime.<hash>.mjs`): the
  bootstrap's text and its policy hash are the host's, so the host would
  have to read a manifest anyway; the query keeps every route as it is.
- **A container platform first** (Fly.io, Cloud Run): its managed
  PostgreSQL is across a network, which the host's driver reaches without
  TLS; kiokun's 1.49 million files want a volume. The image is ready for one.

## Structural guarantees and their evidence

- One answer to "which data layer" (`layer_of`), which the server is built by
  and production is held to.
- One answer to "which assets": `asset_files` lists what the routes serve;
  the table refuses everything else, and `serve_file` refuses in production.
- The bootstrap's text and its policy hash come from one `bootstrap()`.
- Every response head goes through `production::head()` (status for the log,
  production's lines), at the five writers.

## Acceptance

Recorded by `just e14-deploy` in `docs/evidence/E14/deploy.txt`, verified on
GitHub (`verify` run on `wip/deploy`, fetched by `just evidence-fetch`):
- the server's production tests: 42 (production::, the request-level
  `tests::production_host::`, kiokun's cache), each rule with its
  development control;
- the binary as production runs it (`tests/production_process.rs`, 6): its
  refusals as JSON lines and never a secret; HSTS, a Secure cookie, the
  runtime gzipped and immutable at its version; a SIGTERM that finishes an
  answer under way and ends with 0; a stop cut at its grace; an idle
  connection closed at its read bound; and the store on a database of its
  own whose bootstrap is the one script its policy hashes and whose instance
  tokens are the deployment key's (the same after a restart, none shared
  under another key);
- the local production smoke (`deploy/smoke.sh`): the release host, the
  store's production build, PostgreSQL 18.6, a TLS front, Playwright in
  Chromium, Firefox and WebKit, 12 and 3 tests before and after a SIGTERM
  restart (headers, compression, caching, a guest's cart kept, sign-in
  absent, test controls 404, metrics by token, health and readiness,
  migrations not applied again, every log line JSON with no secret);
- the mutation controls (`scripts/deploy_mutations.py`, 60 mutants: 51 of
  the unit suite, 9 of the process): see the evidence for the count killed.

The whole server suite and `just ci` pass at the track's head (the
progress file and the pull request name the run).

## Not claimed

- **The image is unbuilt here** (no container runtime on the PC); its
  Containerfile is checked by reading, its minifier's lock by `npm ci
  --dry-run`. The VM recipe is unrun (no VM); `systemd-analyze verify`
  accepts the unit.
- **No real sign-in**: Google's OIDC needs the owner's OAuth client (NEXT).
- **The database without TLS**: production's database must be on the same
  machine or a private network.
- **No backups, alerts or rate limits**; one host (a second needs
  `track/second-host`'s shared sessions, which a store on PostgreSQL has).
- **Brotli** (no crate); the 30-second read bound's default is tested only
  through its setting.
- A panic's JSON line is not exercised by a test.

## What next work needs

1. The owner's step (`DEPLOY-OWNER-STEP.md`, handoff folder): a VM and DNS.
2. `just deploy-vm kiokun --setup --data` for preview.kiokun.com.
3. OIDC (Google) behind `identity::Provider`; then the store in production.
4. Backups (nightly `pg_dump`), a system-call filter measured against
   Wasmtime, a CI job that builds the image and runs the smoke against it.

## What the capability matrix and KNOWN_LIMITATIONS would say

Production: "A production host mode (ADR-XXXX): refuses development
providers, test controls and keys; secrets from env or systemd credentials;
assets gzipped and immutable at the build's version; JSON logs; graceful
stop; an OCI image and a single-VM recipe; no sign-in yet (OIDC queued); TLS
by Caddy; the database local."

## Found (corrections first)

1. A production deployment could not start (no identity model ran there).
2. The development keys (entries, instance tokens) would have keyed
   production's addresses.
3. `GET /StorePage.html?items=N` rewrote the store's menu for anyone.
4. `/metrics` and `sources.json` were anyone's.
5. The stream's head lacked `nosniff`.
6. A secrets-mode check that refuses group bits would refuse every
   systemd credential (ACL'd 0400 shows as 0440); `%d` does not resolve in
   `Environment=` on systemd 249.
7. kiokun's data, search CSV and cache defaulted to the build machine's paths.
8. `renderer-contract.test.mjs` evaluates runtime functions in a script
   context: a runtime helper they call must be given to it.
