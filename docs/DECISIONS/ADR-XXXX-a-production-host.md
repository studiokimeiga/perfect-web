# ADR-XXXX: a production host refuses what production may not run, bounds what a client can hold, and is packaged to run

Status: proposed by track `deploy` (2026-11-02) under the owner's words of
2026-11-02 ("You can deploy. You can handle credentials.") and the
coordinator's brief; repaired the same day after the integrator's
independent review, whose findings and rulings come first below. The
integrator numbers it (ADR-0396) and lands it by pull request. Milestone:
E14, the production path (NEXT: "a deployment, a container image or a host
target"). Builds on ADR-0285 (kiokun.com is the owner's production target,
served first on a preview origin, no cutover without the owner's go),
ADR-0311 (`PW_ORIGIN`), ADR-0333 (what a page may run), ADR-0258 and
ADR-0389 (identity and session authority), ADR-0369 (sessions kept where the
hosts share them) and ADR-0387 (outbound HTTPS, and its `Secret`).

## Corrections first: the review's findings, and what changed

The review read the track at `c60dba2e` and reproduced each finding against
the binary. Each is repaired with a test at the level it lives, a valid
neighbour and a mutation control killed at an assertion
(`scripts/deploy_mutations.py`).

1. **BLOCKER: a client could end the production host, and "its connections
   are bounded" was false.**
   - *(a) The read bound was each read's.* A byte every 1.5 s held a
     connection past 12 s with `PW_READ_TIMEOUT=2`, and nothing bounded a
     write. **Now** a request arrives by one deadline: its whole head and
     body within `PW_READ_TIMEOUT` (30 s) of its connection's start,
     however its bytes are spaced (`production::connections::Arriving`,
     which gives each read what is left), answered `408` past it (RFC 9110
     §15.5.9); a connection that sent nothing is closed unanswered. An
     answer is written by one deadline, `PW_WRITE_TIMEOUT` (30 s), its head
     and body together (`Sending`, at `main.rs`'s `write_response` and an
     upload's answer), and every other write on the connection (a stream's
     frames) within it (`SO_SNDTIMEO`, `connections::prepare`). Process
     tests: a head that trickles in and a body that trickles in are each
     answered 408 within the deadline; an answer of ten megabytes to a
     client that reads nothing is cut, and to one that reads four
     kilobytes a fifth of a second, which keeps every single write moving,
     its connection is done at the deadline all the same (a stop four
     seconds on finds none open). Thirty seconds lets a feed's 5 MB image
     arrive at 1.4 Mbit/s; Node gives a head sixty and a request three
     hundred, Go's `WriteTimeout` bounds an answer as this does.
   - *(b) No limit on connections*: 2,000 that each declared a body and
     sent none held 2,036 threads. **Now** at most `PW_MAX_CONNECTIONS`
     are open (1,024; a whole number from 2, a page's document and its
     subscription at once, to 65,536; anything else refused at start),
     counted and checked in one step as each is accepted
     (`Gate::try_open`). One more is answered by the accepting thread,
     which starts none for it: `503`, `Retry-After: 1`, `nosniff` and the
     host's HSTS, written without waiting, what the client sent read first
     so the close is no reset (`connections::turn_away`), and said in one
     log line every ten seconds with how many it stands for
     (`connections::Said`). A host whose descriptor limit cannot hold its
     connections (three each, and 256) raises its soft limit to them, as
     Go's runtime does, or is refused at start where its hard limit is
     lower. Process tests: four held open at a limit of four, a fifth is
     answered 503 within two seconds and the process has no more threads
     than before; one closes, and the next is answered; a host started
     with a soft limit of 512 runs with 3,328, and one with a hard limit of
     1,024 is refused by its reason.
   - *(c) `std::thread::spawn` panics where the system refuses a thread*,
     and in the accepting thread that ended the process (exit 101 under
     `prlimit --nproc`), which then refused everything. **Now** each
     connection's thread is started by `std::thread::Builder`; one the
     system refuses is its connection dropped, unanswered, said once in ten
     seconds, and the host lives. The host's own threads at start (the
     clock's, the scheduler's, the other hosts' commits) are refused at
     start where the system gives none, and the host says it listens only
     once they run (the review's mutation was a race with them). Process
     test: the running host's `RLIMIT_NPROC` set to one (prlimit(2)), a
     connection is dropped and said; the limit restored, the next is
     answered (skipped where the tests run as root, whom the limit does not
     hold).
   - *(d) `pleris.service` set no `TasksMax=` or `MemoryMax=`.* **Now**
     `TasksMax=4096` (four times the default limit of connections, for the
     host's own threads and a commit's tellers), `MemoryHigh=768M` (the
     kernel reclaims; kiokun's dictionary is read through the page cache,
     which is charged there and given back) and `MemoryMax=1G` (the process
     ends and `Restart=` starts it): a VM of 2 GB; each is to be raised
     with the VM, less PostgreSQL's and Caddy's. `LimitNOFILE=65536` is
     past 3,328. `systemd-analyze verify` (systemd 249 here) accepts the
     unit.
   - *(e) `/_pw/ready`'s `SELECT 1` waited without end, under a mutex*
     every other check waited behind. **Now** a check runs in a thread of
     its own, one at a time: a question asked while one is under way waits
     for its answer, none waits past two seconds, and no lock is held while
     it waits (a `Condvar`'s wait releases it). The kept connection is asked
     with `Client::is_valid` (two seconds); a new one is given two seconds
     to connect and ten for unacknowledged bytes (`TCP_USER_TIMEOUT`). A
     database that accepts and never answers holds one thread, the check's,
     in its startup (which the driver bounds by nothing), never a
     request's. Unit test: four checks at once against a database that
     never answers each answer within the bound, asking it once between
     them; process test: through a TCP passage the test stalls, four
     readiness checks at once each answer 503 within four seconds, asking
     the database at most two connections' worth, and once it answers again
     so does the host.
   - **Thread a connection is the host's known scaling limit**, which these
     bounds keep alive at its limit and do not raise. Each connection holds
     an OS thread (2 MiB of address space, what it touches of it) for its
     life, a page's subscription among them, so 1,024 connections are some
     500 readers with a page open; past that a page is told 503. A
     connection's own helper threads (a commit's tellers, a render's scoped
     reads) still `scope.spawn`, which panics that connection's thread where
     the system refuses one: the connection is lost, the host lives, and
     `TasksMax=` leaves them room. **What an evented server would change**
     (a decision for later, not now, the integrator's): a connection held as
     a few kilobytes of state on an async runtime (tokio and hyper, both in
     Cargo.lock through other crates) rather than a thread; its deadlines
     timers, not socket options; HTTP/1.1 keep-alive and HTTP/2 without a
     thread a stream; the stop's drain a cancellation. It would rewrite the
     accepting loop, the head's and the body's reading, every writer
     (`write_response`, a page's settling parts, the subscription), the
     data layers' PostgreSQL client (the sync `postgres` to `tokio-
     postgres`), and run Wasmtime's calls on a blocking pool: the host's
     whole request path, which no track of E14's owns today.

2. **Two headline properties could be removed with every deploy test
   passing**: resource entries keyed by the public development key in
   production (`production.rs:345` changed to `entry_key(None)`), and a
   production store with no database started (`main.rs:9293` made never to
   refuse); and `entry_key` fell back to the development keys where none
   was installed. **Now** a deployment's keys are decided once
   (`keys_for`): its own where it states a key, the development server's
   only in development, and in production a refusal, never the
   development keys; `main` installs them before anything is keyed.
   Process tests: one session's resource entries in a production store are
   the same after a restart under the same key, and share nothing with
   another key's or with a development server's (its manifest's
   `"entry"` ids); a production store with no database is refused at
   start, by its reason. Unit tests: production with no key is refused and
   given no keys; the settings carry a deployment's own. The control count
   was wrong too (62 = 52 + 10, said 60): it is 120 now (below).

3. **`release.sh`'s `rsync -a` kept the builder's uid on the VM**, where
   the receiver is root, so the account that built a release could replace
   the binary systemd runs; `PROGRAM` was not checked, and `--setup`'s
   arguments were re-read by the VM's shell. **Now** every copy is
   `rsync -a --chown=root:root --chmod=go-w`; the program is checked
   against the three the recipe builds, the domain as a host name, the
   destination and the key path as names with no option and no space,
   before any machine is asked; the setup's command line is one argument,
   each value quoted for the VM's shell (`printf %q`), and the commit is 40
   hexadecimal digits. `deploy/vm/release-checks.sh` holds each with
   stand-ins for `git`, `ssh`, `rsync` and `cargo` on PATH, so nothing
   reaches a machine or builds: six refusals, exit 2, with no stand-in run;
   and well-formed settings going on to the setup, its copy root's and its
   command line one quoted argument.

4. **The image's documented secrets could not work**: the Containerfile's
   `docker run` mounted a root-owned 0600 file the image's user (65532)
   cannot read; Kubernetes' secret files (0644 by default) and Docker's and
   Podman's (0444) were refused by the mode check, which also read the
   file's metadata by its path and its text by its path again. **Now** the
   secrets file is opened once without following a link (`O_NOFOLLOW`, and
   `O_NONBLOCK`, so a pipe is not waited on), held to the rule by `fstat`
   of the descriptor opened, and read from it: a regular file; its owner
   this process's user or root; no one else may read or write it; its
   group may not write it, and may read it only where the group is this
   process's own (`getegid`); and an access ACL is read for what it grants
   (`system.posix_acl_access`, parsed): a named user may read only where
   it is this process's user, a named group only where it is this
   process's group, none may write, and its mask grants nothing. Unit
   tests cover each, a real ACL set with `fsetxattr` among them.
   **Decision, beside the ruling's letter** ("mode not readable or writable
   by group/other"): a group's read is taken where the group is the
   process's own. Kubernetes cannot give a secret file to `runAsUser`: its
   kubelet ORs `0440` into every file of a read-only volume and gives it
   `fsGroup` (`roMask` in `pkg/volume/volume_linux.go`, read on
   2026-11-02), so a non-root container reads one only through its group;
   systemd's credential passes either way (its ACL's owning-group entry
   grants nothing; where its file system keeps no ACL it gives the file to
   the service's user). The integrator may hold the letter instead, and
   Kubernetes then has no invocation. The invocations that work and pass:
   - **Docker**: `sudo install -o 65532 -g 65532 -m 0400 secrets
     /etc/pleris/secrets`, then `docker run --read-only --cap-drop=ALL
     --security-opt=no-new-privileges -p 127.0.0.1:8080:8080 -e
     PW_ORIGIN=… -e PW_IDENTITY=none -v
     /etc/pleris/secrets:/run/secrets/pleris:ro -e
     PW_SECRETS_FILE=/run/secrets/pleris pleris-store` (a bind mount keeps
     the file's owner and mode; the port on the loopback, TLS being the
     proxy's).
   - **Podman**, rootless (which maps users): `podman secret create pleris
     secrets`, then `--secret
     pleris,type=mount,target=/run/secrets/pleris,uid=65532,gid=65532,mode=0400`
     (its documented defaults are uid 0 and mode 0444, which the host
     refuses).
   - **Compose**: `secrets: [{source: pleris, target: pleris, uid:
     "65532", gid: "65532", mode: 0400}]` with `secrets: {pleris:
     {environment: PLERIS_SECRETS}}`. Compose honours `uid`, `gid` and
     `mode` only for a secret from an `environment` source; one from a
     `file` is bind-mounted as it is and those are "silently ignored" (its
     documentation, read 2026-11-02), so a `file` source is the Docker case
     above.
   - **Kubernetes**: `securityContext: {runAsUser: 65532, runAsGroup:
     65532, fsGroup: 65532, runAsNonRoot: true}`, a volume `secret:
     {secretName: pleris, defaultMode: 0400}`, mounted `mountPath:
     /run/secrets/pleris, subPath: secrets, readOnly: true` (the volume's
     own names are links, which the host does not follow; a `subPath`
     mount is the file), and `PW_SECRETS_FILE=/run/secrets/pleris`. The
     file is then `0440`, group 65532.

5. **The review's notes**, each repaired unless the ruling says record:
   - *(a) An upload wrote its own response head*, without HSTS or its
     status for the log. **Now** it goes through `production::head` with
     every other head, and its writes by the answer's deadline. A source
     test holds every place a server source formats a status line to call
     `production::head` (but the development payment provider's own server,
     another origin refused in production, and the busy answer, which no
     request's thread writes and which carries `production::lines()`).
   - *(b) `GET /menu`, a development server's, answered in production*: a
     deny-list of test routes left it. **Now** a production host serves
     only the program's pages at their routes, its declared commands, its
     uploads' paths, its build's assets, and the platform's own (its health,
     readiness and metrics, a page's subscription, keyed reads and handler
     table, the relying party's routes, the payment provider's word where
     payments are configured), each by the pattern of its route
     (`production::served_as`); anything else, `/menu`, `/StorePage.html`,
     `/page/…` and every test control among them, is `404` with no session.
     And **the compiler refuses a program route under a path the host
     keeps** (PW7001, in a new deploy block, PW70xx): a page's route, or an
     upload's route or the path it serves at, at or under `/_pw`, `/bench`,
     `/command`, `/computed`, `/dev-idp`, `/handler`, `/metrics`,
     `/payments`, `/pw-handlers`, `/pw-read`, `/pw-render.wasm`,
     `/pw-resume.wasm`, `/pw-runtime.mjs`, `/sign-in`, `/sign-out`,
     `/sign-up`, `/speculation`, `/StorePage.html` or `/stream` (segment by
     segment: `/benchmark` and `/menus/{name}` are a program's).
     `pw_core::reserved::RESERVED` and `production::RESERVED` are one list,
     a test holds them equal and every route `main.rs` answers itself under
     one; a build made by other means that names one is refused at start
     (`build_refusals`). **A development server's `/menu`, `/menu-entry` and
     `/page/…` are not reserved: each now yields to a page the program
     declares there** (its arm guarded by `routed.is_none()`), in
     development and in production. Reserving them, as this repair first
     did, refused the guide's own tour (`docs/guide/tour-pages.md`'s
     `MenuPage` at `/menu`) and a compiler test's page: a restaurant's
     program may well have a `/menu`, and a test route of the development
     server's should not take it. Two tests of ADR-0389's, an upload at
     `/command/attach` and at `/stream` admitted before dispatch, now build
     that upload by hand (their `uploads.json` rewritten), since `pw check`
     refuses the program. `docs/guide/diagnostics.md` lists PW7001
     (`scripts/guide_diagnostics.py`).
   - *(c) The 64 KiB head bound took a head of 70,559 bytes whose first
     65,536 ended on a line break*, and read the rest as its body. **Now**
     a head is read to its blank line, which must come within the bound
     too (`read_head`): past it, `431` wherever its lines break; a head of
     exactly 65,536 bytes is answered and one byte more refused. A bare
     `\n` ends a line as `\r\n` does (RFC 9112 §2.2).
   - *(d) Weak keys.* **Now** `PW_IDENTITY_KEY` is refused, in development
     too, where it holds fewer than ten distinct hexadecimal digits (64
     digits of `openssl rand -hex 32` do with a probability under 10⁻¹¹),
     repeats a run of its digits, or is a key this repository publishes
     (its tests' until now, refused by name). Tests make a key from the
     operating system's random source each run, and a test holds that every
     64-digit run the server's sources, the process tests, the deploy files
     and this ADR name beside a key's name is refused.
   - *(e) Privacy.* **Now** the host's request line says the pattern of the
     route that answered (`/word/{word}`, `/stores/{id}`,
     `/images/{key}`), set as the request is routed, and none for what it
     does not serve; never the path, whose words and query are what a
     reader looked up. Caddy's access log and its own log keep no request's
     path or query and no referrer (a `filter` format deleting
     `request>uri` and `request>headers>Referer`; it writes cookies and
     `Authorization` as `REDACTED` itself). A process test holds the
     host's lines to patterns, and the smoke the requests its browsers made.
   - *(f) Stop.* **Now** the signals are waited for from the first line of
     `main`: one that comes while the host starts (its database migrating,
     its data warming) ends the process at once with 0 (a migration is one
     transaction, rolled back with its connection, its advisory lock
     released; kiokun's index is built aside and moved into place whole);
     a second, while the open connections finish, ends it at once; and a
     subscription ends at the stop, a stream at its next pass and a long
     poll with what it has, rather than holding the stop for its passes.
     Process tests: a store whose database accepts and never answers is
     stopped while it starts, within three seconds, never having listened;
     a second SIGTERM ends a stop waiting on a thirty-second grace; a
     stream and a long poll opened just before SIGTERM end, and the
     process within 600 ms.
   - *(g) `IdentityDomain` derived `Debug` and `Serialize` holding the
     production instance key.* **Now** it derives neither (nothing
     serialized one), its key field is private, and its `Debug` names the
     development key or "a deployment's key (not shown)". Tests: no eight
     characters of a key in its `Debug`, `pw-document`'s and the
     production unit suite's, with the key a production host derives.
   - *(h) A database on another machine reached without TLS.* **Now** a
     production host refuses `PW_STORE_DATABASE_URL` or
     `PW_FEED_DATABASE_URL` naming any host but this machine's (a Unix
     socket, `localhost`, a loopback address) unless it requires TLS
     (`sslmode=require`; this driver reads no stronger mode), by its own
     rule, the address never quoted. The host's driver connects without TLS
     (`postgres::NoTls`), so such a database is refused when the host
     connects: a database on another machine waits for a TLS connector
     (Next). A container reaches only its own loopback or a mounted socket
     (`--network host`, a pod's sidecar).
   - *(i) The smoke's secret scan matched only whole values.* **Now** it
     looks for every eight characters of each secret, and of the password a
     database's address holds, which a role made for the run gives (the
     admin's, `pw`, is too short to look for); an address is looked for
     whole (its host and name are no secret).
   - *(j) Caddy's admin API.* **Now** `admin off`: its endpoint took a new
     configuration from any process on the VM. A change of configuration
     is a restart (setup.sh restarts Caddy; `caddy reload` needs the API).
   - *(k) Recorded, not changed.* **The `__Host-` cookie prefix**: the
     session cookie is `pw-session` (Secure, HttpOnly, SameSite=Lax,
     Path=/); `__Host-pw-session` would refuse one a sibling host sets
     (RFC 6265bis §4.1.3.2). Not changed here: `identity.rs` is being
     changed by two other workers; Next. **Rolling back over a migrated
     schema**: a host applies the migrations it knows, each once, and
     refuses none it does not know, so `release.sh`'s put-back starts the
     release before on the schema this one migrated and says it is ready
     whatever its queries then meet. The rule an operator follows (in
     `release.sh`'s header): a migration a release adds is one the release
     before reads past (a new table, a new column with a default, a new
     index); changing or removing what an older release reads waits for a
     release after the one that stops reading it (expand, then contract);
     a release whose migration is not so is rolled forward, never back.

6. **Evidence**: `docs/evidence/E14/deploy.txt` was missing. `just
   e14-deploy` now records everything this ADR claims (Acceptance), and
   the integrator records it from CI at the track's head.

## Context

- **The host is the development server** ("NOT E8's production host" in its
  own Cargo.toml; capabilities.md's Production row: "Pieces, not a
  deployment"). `PW_DEPLOYMENT=production` and `PW_ORIGIN` existed, a
  generated policy, migrations at start under an advisory lock, `/metrics`.
- **What was read before this track** (2026-11-02, master 27c1ffba): a
  production deployment could not start (both identity models refuse
  anything but a development deployment on this machine); its addresses
  were keyed by constants this public repository holds; test controls
  answered anyone, and `GET /StorePage.html?items=N` rewrote the store's
  menu; `/metrics` and `sources.json` were anyone's; assets were read from
  disk for each request, uncompressed, with no cache headers; it bound
  127.0.0.1 alone, read a request's head without bound, waited for a read
  without end, and stopped only when killed; logs were lines of text on two
  streams; kiokun's data, search index and cache defaulted to compile-time
  paths.
- **No deploy tooling or cloud credential exists on the development PC**
  (no Docker, Podman, Caddy, flyctl, shellcheck).

## Decision

1. **`PW_DEPLOYMENT=production` refuses at start, every reason said at
   once** (`production.rs`, `Settings::read`, `program_refusals`,
   `build_refusals`): an origin that is not `https://`; no key of its own,
   or one that is no secret's (correction 5d); a test's setting
   (`PW_FEED_TRANSACTIONS=connection`); a store or a feed with no database,
   a feed's uploads with no `PW_BLOB_DIR`, kiokun with no `KIOKUN_DATA` or
   no `PW_CACHE_DIR`; a database on another machine without TLS; a build
   serving a path the platform keeps; a setting it cannot read
   (`PW_BIND`, `PW_SHUTDOWN_GRACE`, `PW_READ_TIMEOUT`, `PW_WRITE_TIMEOUT`,
   `PW_MAX_CONNECTIONS`); a descriptor limit below its connections. The
   development identity models and payment provider are refused where they
   are configured, as before.
2. **The one identity production can run today is `PW_IDENTITY=none`**: no
   sign-in, every session anonymous (`requires SignedIn` refuses), and
   `/sign-in`, `/sign-up` and the callback answered 404 with a page that
   says so. A guest's cart is a session's, as ADR-0318 has it.
3. **Its addresses are keyed by the deployment's secret**: `PW_IDENTITY_KEY`
   (32 bytes or more, hexadecimal, no weak or published key) split by
   BLAKE3's `derive_key`, one context for resource entries and one for
   instance tokens (`keys_for`, `install_keys`): never the development keys
   in production.
4. **Its secrets are read from the environment or a secrets file**
   (`production/secrets.rs`): the names are the host's (`PW_IDENTITY_KEY`,
   `PW_STORE_DATABASE_URL`, `PW_FEED_DATABASE_URL`, `PW_METRICS_TOKEN`); the
   file is `PW_SECRETS_FILE`'s, or systemd's credential `secrets` in
   `$CREDENTIALS_DIRECTORY` (`LoadCredential=`); it is held to correction
   4's rule; a secret given twice is refused; no refusal quotes a line of
   the file; `Secret` has no `Display` and a redacted `Debug`. ADR-0387's
   `Secret` cannot be used: its value is readable only inside `pw-outbound`.
5. **It serves the program's routes and the platform's own, nothing else**
   (`served_as`, correction 5b): `/metrics` only to a bearer of
   `PW_METRICS_TOKEN` (compared in constant time); without one, no one.
6. **It serves its build's assets from memory** (`production/assets.rs`):
   the build root's `.mjs`, `.js`, `.wasm`, `.css`, `.svg` (never a page or
   a `.json`), each handler's, speculation's and computed module; a build
   missing one is refused at start. Each has a strong `ETag` (its SHA-256;
   another for its gzip, RFC 9110 §8.8.3) and `If-None-Match` is answered
   304. **Hashed addresses**: the build's version is a digest of every
   asset; documents start the runtime at `/pw-runtime.mjs?v=<version>`, and
   the runtime carries the same `v` to its WebAssembly, its handlers and
   its own re-import (`asset()` in `pw-runtime.mjs`). An answer at this
   build's `v` is `public, max-age=31536000, immutable` (RFC 8246); any
   other `no-cache`.
7. **Compression: gzip, assets only**, once at start (miniz_oxide's DEFLATE
   at level 9, crc32fast's CRC-32: both in Cargo.lock) where that is
   smaller, with `Vary: Accept-Encoding`. **A session's answer is never
   compressed** (BREACH). Brotli is not here: no crate in Cargo.lock writes
   it.
8. **The runtime is minified** by esbuild 0.28.2: 158,077 to 50,078 bytes,
   gzipped 49,611 to 17,610. `deploy/build-dist.sh` builds one program's
   production `dist`; the image installs the same esbuild with `npm ci`
   from `deploy/minify/package-lock.json`, written from pnpm-lock.yaml's
   SHA-512s by `scripts/deploy_minify_lock.py`.
9. **It says whether it lives and is ready**: `GET /_pw/health` (always,
   `no-store`) and `GET /_pw/ready` (503 while it stops, or where its
   database does not answer within two seconds, correction 1e).
10. **Every answer carries `Strict-Transport-Security: max-age=31536000`**
    (not `includeSubDomains`), through `production::head`, every head's.
11. **TLS is the proxy's or the platform's, not the host's**: Caddy on a VM
    (`deploy/vm/Caddyfile`: automatic certificates, HTTP/2 and HTTP/3 to
    browsers, a restart waited for, no admin API, no request's path in any
    log), a platform's edge elsewhere. The host closes every connection
    after one answer (HTTP/1.1): Caddy's HTTP/2 multiplexes browsers'
    requests over it.
12. **JSON lines**: in production every line the host writes is one JSON
    object on standard error (`ts`, `level`, `event`, `msg`), its refusals,
    panics and trace among them, and one a request (method, the route's
    pattern, status, milliseconds; a subscription's and a health check's
    only where they failed). Development's lines are unchanged.
13. **It stops when told** (correction 5f): SIGTERM or SIGINT, blocked in
    every thread from the first and taken by `sigwait` in one: during the
    start, the end of the process; once listening, no new connection (a
    connection of its own wakes `accept`, the listener closed at once so a
    connection made then is refused and Caddy retries it), subscriptions
    ended, the open connections waited for up to `PW_SHUTDOWN_GRACE` (8 s),
    then 0; a second signal, the end at once.
14. **Its connections are bounded** (correction 1): a request's head at
    most 64 KiB (431 past it, in development too); in production a
    request's head and body by one deadline, an answer by another, at most
    `PW_MAX_CONNECTIONS` open, a refused thread a dropped connection.
15. **The deployable artifacts** (`deploy/`): `Containerfile` (node stage
    for esbuild; rust:1.97.1-bookworm stage for the host, `pw`, the
    WebAssembly and `build-dist.sh`; distroless `cc-debian12:nonroot`,
    read-only, no shell; each base pinned by its index digest, read from
    its registry on 2026-11-02: rust `sha256:0e2bcaef…4a97`, node 22.21.1
    `sha256:25b3eb23…ae2c`, distroless `sha256:9dac0a79…182f`), a root
    `.dockerignore`; and the single-VM recipe `vm/`: `setup.sh`
    (PostgreSQL 18 from PGDG with peer authentication; Caddy from its
    stable repository; the `pleris` user; settings; secrets made once on
    the VM; unattended upgrades; a firewall), `pleris.service`
    (`LoadCredential`, `ProtectSystem=strict` and the rest of systemd's
    confinement, `TasksMax=`, `MemoryHigh=`, `MemoryMax=`; not
    `MemoryDenyWriteExecute`, which Cranelift's compilation at start
    needs), `Caddyfile`, `release.sh` (a clean commit built here, copied as
    root's, switched, checked at `/_pw/ready`, put back if not ready) and
    `release-checks.sh`. `just deploy-dist|deploy-image|deploy-smoke|
    deploy-vm`. Nothing reaches a machine without the owner's
    `~/.config/pleris/deploy.env`.

## Alternatives

- **TLS in the host (rustls)**: no certificate issuance or renewal without
  an ACME crate, no HTTP/2; rejected for the proxy (decision 11).
- **Compressing documents too** (or Caddy's `encode`): BREACH; rejected.
- **ETag and `no-cache` alone, no versioned address**: correct, but a round
  trip per asset per page.
- **A deny-list of test routes** (this track's first answer): the review
  found `GET /menu` past it; an allow-list fails closed (correction 5b).
- **Per-read and per-write timeouts** (nginx's `client_body_timeout`,
  `send_timeout`): a client that keeps bytes coming holds a thread for
  good; one deadline a request and one an answer (Go's `ReadTimeout`,
  `WriteTimeout`; Node's `requestTimeout`) do not (correction 1a).
- **Not accepting past the limit** (leaving connections in the backlog):
  the platform's health check would wait with every other; a cheap 503 says
  when to ask again (correction 1b).
- **An evented server**: correction 1's last point; a decision for later.
- **A container platform first** (Fly.io, Cloud Run): its managed
  PostgreSQL is across a network, which the host reaches only with TLS it
  does not speak yet (correction 5h); kiokun's 1.49 million files want a
  volume. The image is ready for one.

## Structural guarantees and their evidence

- One answer to "which data layer" (`layer_of`), which the server is built
  by and production is held to.
- One answer to "which keys" (`keys_for`): production's are its own or it
  does not start.
- One answer to "which assets": `asset_files` lists what the routes serve;
  the table refuses everything else, and `serve_file` refuses in production.
- One answer to "what production serves" (`served_as`), and one list of the
  paths the platform keeps, the compiler's and the host's held equal by a
  test, every route `main.rs` answers itself held under it by another.
- The bootstrap's text and its policy hash come from one `bootstrap()`.
- Every response head goes through `production::head()` (status for the log,
  HSTS), held by a source test.
- A connection is counted and held to the limit in one step
  (`Gate::try_open`), and released by its guard whichever way its thread
  ends, or its thread is not started.

## Acceptance

Recorded by `just e14-deploy` in `docs/evidence/E14/deploy.txt`, verified on
GitHub (a `verify` run on `wip/deploy`, fetched by `just evidence-fetch`):
- the server's production tests (`-- production`: `production::`,
  `tests::production_host::`, kiokun's cache), 71, each rule with its
  neighbour; and the two of ADR-0389's for an upload under a path the
  platform keeps;
- the compiler's PW7001 tests (`--test reserved_routes`, 3) and its code
  block (`codes::tests::`), and a domain's `Debug` (`pw-document`);
- the binary as production runs it (`tests/production_process.rs`, 18): its
  refusals as JSON lines and never a secret, a secrets file by a link
  refused; HSTS, a Secure cookie, the runtime gzipped and immutable at its
  version, requests said by their routes' patterns; a SIGTERM that finishes
  an answer under way and ends with 0; a stop cut at its grace; an idle
  connection closed at its read bound; correction 1's bounds, 2's keys and
  refusal, and 5f's stops; the store on databases of its own;
- the unit as `systemd-analyze verify` reads it, and the deploy scripts as
  `shellcheck` does, where each is installed (the development PC has no
  shellcheck);
- what `release.sh` hands a machine, and refuses to
  (`deploy/vm/release-checks.sh`);
- the local production smoke (`deploy/smoke.sh`): the release host, the
  store's production build, PostgreSQL 18, a role whose password is made
  for the run, a TLS front, Playwright in Chromium, Firefox and WebKit, 12
  and 3 tests before and after a SIGTERM restart (headers, compression,
  caching, a guest's cart kept, sign-in absent, the development server's
  routes and test controls 404, metrics by token, health and readiness,
  migrations not applied again, every log line JSON with no eight
  characters of a secret, each request by its route's pattern);
- the mutation controls (`scripts/deploy_mutations.py`, 120 mutants: 85 of
  the unit suite, 25 of the process suite, 4 of the compiler's and 6 of the
  release checks): the evidence says how many are killed, each at an
  assertion.

The whole server suite and `just preflight` pass at the track's head (the
integrator's run names it).

## Not claimed

- **The image is unbuilt here** (no container runtime on the PC); its
  Containerfile is checked by reading, its minifier's lock by `npm ci
  --dry-run`. The VM recipe is unrun (no VM); its Caddyfile is checked
  against Caddy's documentation, not by Caddy (none installed).
- **No real sign-in**: Google's OIDC needs the owner's OAuth client (Next).
- **No database on another machine**: correction 5h refuses one without
  TLS, and the host's driver does not speak TLS yet.
- **No backups, alerts or rate limits beyond the connection limit**; one
  host (a second needs `track/second-host`'s shared sessions, which a store
  on PostgreSQL has).
- **Thread a connection** (correction 1's last point): 1,024 connections,
  some 500 readers with a page open; a connection's helper threads still
  panic that connection's thread where the system refuses them.
- **The readiness check's startup is unbounded by the driver**: a database
  that accepts and never answers holds the check's one thread until it
  answers (every question is answered within two seconds meanwhile).
- **Brotli** (no crate); a panic's JSON line is not exercised by a test.
- **The `__Host-` prefix and the rollback rule are recorded, not
  enforced** (correction 5k).
- **The deploy block PW70xx is the track's choice** (Questions).

## Next

1. The owner's step (`DEPLOY-OWNER-STEP.md`, handoff folder): a VM and DNS.
2. `just deploy-vm kiokun --setup --data` for preview.kiokun.com.
3. OIDC (Google) behind `identity::Provider`; then the store in production;
   then the session cookie as `__Host-pw-session`.
4. A TLS connector for the host's PostgreSQL client (rustls is in
   Cargo.lock since ADR-0387), so a managed database can be named.
5. Backups (nightly `pg_dump`), a system-call filter measured against
   Wasmtime, a CI job that builds the image and runs the smoke against it,
   and a migration check that refuses a schema newer than the binary.
6. Whether to move the host to an evented server (correction 1).

## Questions for the integrator

1. **The deploy track's code block**: PW7001 sits in PW70xx, the first
   unclaimed block (PW67xx is ADR-0339's, PW69xx ADR-0384's), owner
   `Owner::Deploy`, held by `a_tracks_block_holds_only_its_tracks_codes`.
   Renumbering is a constant and a test's line.
2. **A group's read of the secrets file** (correction 4's decision): taken
   where the group is the process's own, for Kubernetes' sake; the ruling's
   letter refuses it.

## What the capability matrix and KNOWN_LIMITATIONS would say

Production: "A production host mode (ADR-XXXX): refuses development
providers, test controls, weak keys and a database elsewhere without TLS;
serves the program's routes and the platform's own, nothing else; bounds a
request, an answer and its connections; secrets from env or systemd
credentials, held to their owner; assets gzipped and immutable at the
build's version; JSON logs by route pattern; a stop heard from the first; an
OCI image and a single-VM recipe; no sign-in yet (OIDC queued); TLS by
Caddy; the database local; a thread a connection."

## Found (the track's own findings, before the review)

1. A production deployment could not start (no identity model ran there).
2. The development keys (entries, instance tokens) would have keyed
   production's addresses.
3. `GET /StorePage.html?items=N` rewrote the store's menu for anyone.
4. `/metrics` and `sources.json` were anyone's.
5. The stream's head lacked `nosniff`.
6. A secrets-mode check that refuses group bits would refuse every
   systemd credential (ACL'd 0400 shows as 0440); `%d` does not resolve in
   `Environment=` on systemd 249.
7. kiokun's data, search CSV and cache defaulted to the build machine's
   paths.
8. `renderer-contract.test.mjs` evaluates runtime functions in a script
   context: a runtime helper they call must be given to it.

And in the repair: the host said it listened before it had started its own
threads, so a limit set the moment it said so could end it (found by the
refused-thread test's first runs); the session tests that declared an upload
under `/command/` and at `/stream` were programs the platform's own routes
shadow (a subscription's `/stream` among them), which PW7001 now refuses.
