#!/usr/bin/env python3
"""Mutation controls for track deploy (ADR-XXXX, a production host): what
`PW_DEPLOYMENT=production` refuses at start and answers itself, and what
bounds its connections.

Each mutant undoes one piece:
- the start: an origin that is not https, or none, taken; production given
  the development keys where it states none; a key of fewer than 64 digits,
  or one whose digits are no secret's (few distinct, a run repeated, one
  this repository publishes), taken; a test's setting taken; a development
  server listening anywhere; a setting out of its range taken; the write
  bound not read; a store, a feed's uploads or kiokun whose data would be
  the process's taken; a database on another machine reached without TLS,
  or one that requires TLS refused; a build serving a path the platform
  keeps taken;
- the keys: the development key kept for resource entries, the instance
  tokens keyed with the entries' key or the development one;
- the answers: anything production does not serve answered, a command the
  program does not declare served, the handler table, the program's pages
  or an upload's paths not served, a path the platform keeps that the
  compiler does not refuse, a development server's own route shadowing the
  program's page there; a stopping host or one whose database does not
  answer said ready; a readiness check that waits without bound, holds its
  lock while it waits, or asks the database once for each question;
  metrics read with no token, by a token's prefix, or by a token of another
  length; the host's own answers or its file refusal skipped; a head past
  its bound taken where it ends on a line break, or cut mid-line;
- the connections: a request read with a deadline for each read, an answer
  written with one for each write; a gate taking one past its limit; a
  turned-away answer with no Retry-After; a failure said each time;
- the assets: an older build's version, or any address, kept for good;
  gzip sent unasked, or at a weight of zero; a gzip no smaller kept; no
  `Vary`; one tag for both representations; `If-None-Match` not read; a
  version blind to a digest; a member's CRC-32 or length not its bytes';
- the secrets: a file anyone may read, or its group may write, read; a file
  of another owner read; its group's read where the group is not this
  process's; a link followed; a pipe waited on; a file that is no regular
  file read; an ACL not read, its named user taken as this process's, its
  other entry read as nothing; a secret given twice taken; a line that is
  not `NAME=value` quoted; a secret's `Debug` showing it; systemd's
  credential not read; a domain's `Debug` showing its key;
- the log: a routine request said, a failed one not; a month off by one;
  a request's route kept as none;
- the stop: a drain that waits for nothing, a closed connection not
  counted, the wake sent to an address nothing listens on, a second
  signal draining again;
- no sign-in: a sign-in that falls through, every session signed in;
- kiokun's search cache not where the deployment keeps it, and its index
  not opened before its first read;
- every response head through `production::head`: an upload's not;
- the process (the binary run as production runs it): no HSTS, the runtime
  started at no version, the deployment's key not installed, production's
  entries keyed by the development key, a store with no database started,
  lines not JSON, its data not made ready before it listens, a stop that
  does not drain, an open connection not waited for, no read bound, a
  request's deadline per read, an answer's per write, a late head
  answered nothing, the limit not held, a connection past it held
  unanswered, a refused thread ending the host, the descriptors not
  raised, a readiness check without bound, a stop while it starts drained
  instead of ended, a second signal draining again, a subscription holding
  the stop, a request said by its path, a stopping host taking
  connections, and the listener on the loopback whatever PW_BIND says;
- the compiler: a page or an upload under a path the platform keeps taken,
  a path taken as reserved because it begins with one's letters, a route's
  query part read as its path, an upload's paths not read (PW7001);
- the release: a program, a domain, a destination or a key path not
  checked before release.sh reaches a machine, and its copy keeping the
  builder's uid (`deploy/vm/release-checks.sh`).

A unit mutant must fail the server's tests of production (`cargo test -p
pw-dev-server --bin pw-dev-server -- production`); a process mutant, the
binary's (`--test production_process`, which runs the store on a database
of its own where PW_STORE_DATABASE_URL names a server); a compiler mutant,
`pw-core`'s `--test reserved_routes`. Each kill names the tests that
failed. Only complete native results earn a kill: a build that fails, a
producer that times out, an inventory that differs from the baseline's, or
a failure anywhere but an assertion of a test source is REFUSED, never a
kill. A release mutant must fail `deploy/vm/release-checks.sh` at one of
its own checks (a `FAIL:` line and exit 1); a script that errs otherwise
(bash's own error, another exit) is REFUSED.

Run from the repository root; `just e14-deploy` records the output. The
source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server"
SRC = SERVER / "src"
PRODUCTION = SRC / "production.rs"
ASSETS = SRC / "production/assets.rs"
CONNECTIONS = SRC / "production/connections.rs"
SECRETS = SRC / "production/secrets.rs"
LOG = SRC / "production/log.rs"
SHUTDOWN = SRC / "production/shutdown.rs"
MAIN = SRC / "main.rs"
IDENTITY = SRC / "identity.rs"
KIOKUN = SRC / "kiokun.rs"
UPLOADS = SRC / "uploads.rs"
DOMAIN = ROOT / "runtime/pw-document/src/domain.rs"
RESERVED = ROOT / "compiler/pw-core/src/reserved.rs"
RELEASE = ROOT / "deploy/vm/release.sh"

UNIT, PROCESS, COMPILER, SCRIPT = "unit", "process", "compiler", "script"

# (what is undone, the suite that kills it, file, anchor, replacement)
MUTANTS = [
    # --- the start ---------------------------------------------------------
    (
        "an origin that is not https is taken",
        UNIT,
        PRODUCTION,
        '        Some(origin) if !origin.starts_with("https://") => refused.push(format!(',
        '        Some(origin) if false && !origin.starts_with("https://") => refused.push(format!(',
    ),
    (
        "a deployment that states no origin is taken",
        UNIT,
        PRODUCTION,
        '        None => refused.push(\n            "a production deployment states its public origin',
        '        None => drop(\n            "a production deployment states its public origin',
    ),
    (
        "production is given the development keys where it states none",
        UNIT,
        PRODUCTION,
        "        None if production => Err(",
        "        None if false => Err(",
    ),
    (
        "a key of fewer than 64 digits is taken",
        UNIT,
        PRODUCTION,
        "        let digits = text.len() >= 64",
        "        let digits = text.len() >= 2",
    ),
    (
        "a key whose digits are no secret's is taken",
        UNIT,
        PRODUCTION,
        "        if let Some(why) = weak(text) {",
        "        if let Some(why) = weak(text).filter(|_| false) {",
    ),
    (
        "a key of few distinct digits is taken",
        UNIT,
        PRODUCTION,
        "    if distinct < DISTINCT_DIGITS {",
        "    if distinct < 2 {",
    ),
    (
        "a key that repeats a run of its digits is taken",
        UNIT,
        PRODUCTION,
        "    if (1..=bytes.len() / 2).any(",
        "    if (1..=1).any(",
    ),
    (
        "a key this repository publishes is taken",
        UNIT,
        PRODUCTION,
        "    if PUBLISHED.contains(&text.as_str()) {",
        "    if false && PUBLISHED.contains(&text.as_str()) {",
    ),
    (
        "a test's setting is taken in production",
        UNIT,
        PRODUCTION,
        "        if env(name).as_deref() == Some(*value) {",
        "        if env(name).as_deref() == Some(*value) && false {",
    ),
    (
        "a development server listens anywhere",
        UNIT,
        PRODUCTION,
        "                Ok(address) if production || address.is_loopback() => address,",
        "                Ok(address) if true || production => address,",
    ),
    (
        "a setting out of its range is taken",
        UNIT,
        PRODUCTION,
        "            Ok(n) if (low..=high).contains(&n) => n,",
        "            Ok(n) if low <= high => n,",
    ),
    (
        "any number of connections is taken",
        UNIT,
        PRODUCTION,
        "const CONNECTIONS: (u64, u64) = (2, 65_536);",
        "const CONNECTIONS: (u64, u64) = (0, u64::MAX);",
    ),
    (
        "the write bound is not read",
        UNIT,
        PRODUCTION,
        '            "PW_WRITE_TIMEOUT",',
        '            "PW_NO_SUCH_SETTING",',
    ),
    (
        "a program whose data would be in memory is taken",
        UNIT,
        PRODUCTION,
        "        && secrets.get(name).is_none()\n",
        "        && secrets.get(name).is_none()\n        && false\n",
    ),
    (
        "uploads without their directory are taken",
        UNIT,
        PRODUCTION,
        '    if uploads && env("PW_BLOB_DIR").is_none() {',
        '    if uploads && env("PW_BLOB_DIR").is_none() && false {',
    ),
    (
        "kiokun without its files or its cache is taken",
        UNIT,
        PRODUCTION,
        "    if layer == Layer::Kiokun {",
        "    if layer == Layer::Kiokun && false {",
    ),
    (
        "a database on another machine is reached without TLS",
        UNIT,
        PRODUCTION,
        "    if local {\n        Ok(())",
        "    if true {\n        Ok(())",
    ),
    (
        "a database that requires TLS is refused",
        UNIT,
        PRODUCTION,
        "    if config.get_ssl_mode() == postgres::config::SslMode::Require {",
        "    if false {",
    ),
    (
        "a build serving a path the platform keeps is taken",
        UNIT,
        PRODUCTION,
        "        .filter(|(_, path)| reserved(path))",
        "        .filter(|(_, path)| false && reserved(path))",
    ),
    # --- the keys ----------------------------------------------------------
    (
        "resource entries keep the development key",
        UNIT,
        PRODUCTION,
        "            pw_resource::ExplicitIdentityKey(keys.entries.clone()).identity_key()",
        "            pw_resource::DevelopmentIdentityKey.identity_key()",
    ),
    (
        "instance tokens are keyed with the entries' key",
        UNIT,
        PRODUCTION,
        "            instances: hex(blake3::derive_key(INSTANCES_CONTEXT, &bytes)),",
        "            instances: hex(blake3::derive_key(ENTRIES_CONTEXT, &bytes)),",
    ),
    (
        "instance tokens keep the development key",
        UNIT,
        PRODUCTION,
        "        Keys::Deployment(keys) => keys.instances.as_str(),",
        "        Keys::Deployment(_) => DEVELOPMENT_INSTANCE_KEY,",
    ),
    # --- the answers -------------------------------------------------------
    (
        "a path production does not serve is answered",
        UNIT,
        PRODUCTION,
        "        let Some(pattern) = served_as(server, &self.assets, method, route) else {",
        "        let Some(pattern) = served_as(server, &self.assets, method, route)\n"
        "            .or(Some(String::new())) else {",
    ),
    (
        "a command the program does not declare is served",
        UNIT,
        PRODUCTION,
        "        && server.components.contains_key(id)\n",
        "        && (true || server.components.contains_key(id))\n",
    ),
    (
        "the handler table is not served",
        UNIT,
        PRODUCTION,
        '    "/pw-read",\n    "/pw-handlers",\n',
        '    "/pw-read",\n',
    ),
    (
        "the program's pages are not served",
        UNIT,
        PRODUCTION,
        "        .find(|pattern| crate::route_params(crate::route_path(pattern), route).is_some())",
        "        .find(|_| false)",
    ),
    (
        "an upload's paths are not served",
        UNIT,
        PRODUCTION,
        "    if let Some(pattern) = server.uploads.pattern(method, route) {",
        "    if let Some(pattern) = None::<String> {",
    ),
    (
        "the host keeps a path the compiler does not refuse",
        UNIT,
        PRODUCTION,
        '    "/StorePage.html",\n    "/stream",\n];',
        '    "/StorePage.html",\n];',
    ),
    (
        "a development server's own route shadows the program's page",
        UNIT,
        MAIN,
        '        ("GET", "/menu") if routed.is_none() => {',
        '        ("GET", "/menu") => {',
    ),
    (
        "a stopping host is ready",
        UNIT,
        PRODUCTION,
        '        if gate.draining() {\n            return Err("the host is stopping");',
        '        if gate.draining() && false {\n            return Err("the host is stopping");',
    ),
    (
        "a host whose database does not answer is ready",
        UNIT,
        PRODUCTION,
        "            && let Err(why) = ping.answers()\n",
        "            && let Err(why) = ping.answers().or(Ok::<(), String>(()))\n",
    ),
    (
        "a readiness check waits for its database without bound",
        UNIT,
        PRODUCTION,
        "                .wait_timeout(checks, left)",
        "                .wait_timeout(checks, Duration::from_secs(3600))",
    ),
    (
        "a readiness check holds its lock while it waits",
        UNIT,
        PRODUCTION,
        "                .wait_timeout(checks, left)",
        "                .wait_timeout(checks, Duration::ZERO)\n"
        "                .map(|waited| {\n"
        "                    std::thread::sleep(left);\n"
        "                    waited\n"
        "                })",
    ),
    (
        "every readiness question asks the database itself",
        UNIT,
        PRODUCTION,
        "        if !checks.under_way {",
        "        if true {",
    ),
    (
        "metrics are read with no token",
        UNIT,
        PRODUCTION,
        "        let Some(token) = &self.metrics else {\n            return false;",
        "        let Some(token) = &self.metrics else {\n            return true;",
    ),
    (
        "a token is taken by its prefix",
        UNIT,
        PRODUCTION,
        "        same(given.trim().as_bytes(), token.expose().as_bytes())",
        "        token.expose().starts_with(given.trim())",
    ),
    (
        "a token of another length is the same",
        UNIT,
        PRODUCTION,
        "    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0",
        "    a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0",
    ),
    (
        "a production host's own answers are skipped",
        UNIT,
        MAIN,
        "        && production.answer(server, method, path, &headers, &mut stream)\n",
        "        && false\n        && production.answer(server, method, path, &headers, &mut stream)\n",
    ),
    (
        "a production host serves its build's other files",
        UNIT,
        MAIN,
        '    if server.production.get().is_some() {\n        respond_build(stream, 404, "text/plain", b"not found");',
        '    if server.production.get().is_some() && false {\n        respond_build(stream, 404, "text/plain", b"not found");',
    ),
    (
        "a head that reaches its bound on a line break is taken",
        UNIT,
        MAIN,
        "        if left == 0 {\n            return Head::TooLarge;",
        "        if left == 0 && false {\n            return Head::TooLarge;",
    ),
    (
        "a head past its bound within a line is cut, not refused",
        UNIT,
        MAIN,
        "            return if used == MAX_HEAD {\n                Head::TooLarge",
        "            return if used == MAX_HEAD && false {\n                Head::TooLarge",
    ),
    # --- the connections ---------------------------------------------------
    (
        "a request's deadline is each read's",
        UNIT,
        CONNECTIONS,
        '                .set_read_timeout(Some(left(until, "the request")?))?;',
        "                .set_read_timeout(Some(Duration::from_secs(30)))?;\n"
        "            let _ = until;",
    ),
    (
        "an answer's deadline is each write's",
        UNIT,
        CONNECTIONS,
        '                .set_write_timeout(Some(left(until, "the answer")?))?;',
        "                .set_write_timeout(Some(Duration::from_secs(30)))?;\n"
        "            let _ = until;",
    ),
    (
        "a gate takes one past its limit",
        UNIT,
        SHUTDOWN,
        "        if *open >= limit {",
        "        if *open > limit {",
    ),
    (
        "a host at its limit says no Retry-After",
        UNIT,
        CONNECTIONS,
        r"\r\nretry-after: 1\r\n",
        r"\r\n",
    ),
    (
        "a failure is said every time",
        UNIT,
        CONNECTIONS,
        "        if last.is_some_and(|at| now.duration_since(at) < SAID_EVERY) {",
        "        if false {",
    ),
    # --- the assets --------------------------------------------------------
    (
        "an older build's version is kept for good",
        UNIT,
        ASSETS,
        '            .any(|p| p.strip_prefix("v=") == Some(self.version.as_str()));',
        '            .any(|p| p.starts_with("v="));',
    ),
    (
        "every address is kept for good",
        UNIT,
        ASSETS,
        "        let cache = if versioned { KEPT } else { ASKED };",
        "        let cache = if versioned || true { KEPT } else { ASKED };",
    ),
    (
        "gzip is sent unasked",
        UNIT,
        ASSETS,
        "        let gzipped = asset.gzip.as_ref().filter(|_| accepts_gzip(headers));",
        "        let gzipped = asset.gzip.as_ref();",
    ),
    (
        "a weight of zero accepts gzip",
        UNIT,
        ASSETS,
        "        Some(weight) => weight > 0.0,",
        "        Some(weight) => weight >= 0.0,",
    ),
    (
        "a gzip no smaller is kept",
        UNIT,
        ASSETS,
        "            let gzip = Some(gzip(&bytes)).filter(|g| g.len() < bytes.len());",
        "            let gzip = Some(gzip(&bytes));",
    ),
    (
        "a compressed asset says no Vary",
        UNIT,
        ASSETS,
        '            "vary: accept-encoding\\r\\n"',
        '            ""',
    ),
    (
        "both representations share one tag",
        UNIT,
        ASSETS,
        '            Some(_) => format!("\\"{}-gz\\"", asset.digest),',
        '            Some(_) => format!("\\"{}\\"", asset.digest),',
    ),
    (
        "If-None-Match is not read",
        UNIT,
        ASSETS,
        "        if holds(headers, &asset.digest) {",
        "        if holds(headers, &asset.digest) && false {",
    ),
    (
        "the version reads no digest",
        UNIT,
        ASSETS,
        "            all.update(asset.digest.as_bytes());",
        "            let _ = &asset.digest;",
    ),
    (
        "a member's CRC-32 is of nothing",
        UNIT,
        ASSETS,
        "    member.extend(crc32fast::hash(bytes).to_le_bytes());",
        "    member.extend(crc32fast::hash(&[]).to_le_bytes());",
    ),
    (
        "a member's length is not its bytes'",
        UNIT,
        ASSETS,
        "    member.extend((bytes.len() as u32).to_le_bytes());",
        "    member.extend(0u32.to_le_bytes());",
    ),
    # --- the secrets -------------------------------------------------------
    (
        "a secrets file anyone may read is read",
        UNIT,
        SECRETS,
        "    if mode & 0o007 != 0 {",
        "    if mode & 0o007 != 0 && false {",
    ),
    (
        "a secrets file its group may write is read",
        UNIT,
        SECRETS,
        "        if perm & WRITE != 0 {",
        "        if perm & WRITE != 0 && false {",
    ),
    (
        "a secrets file a group not this process's may read is read",
        UNIT,
        SECRETS,
        "        if perm & READ != 0 && !ours {",
        "        if perm & READ != 0 && !ours && false {",
    ),
    (
        "a secrets file of another owner is read",
        UNIT,
        SECRETS,
        "    if access.owner != uid && access.owner != 0 {",
        "    if false {",
    ),
    (
        "a secrets file's link is followed",
        UNIT,
        SECRETS,
        "        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)",
        "        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)",
    ),
    (
        "a secrets file that is a pipe is waited on",
        UNIT,
        SECRETS,
        "        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)",
        "        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)",
    ),
    (
        "a secrets file that is no regular file is read",
        UNIT,
        SECRETS,
        "    if !meta.file_type().is_file() {",
        "    if false {",
    ),
    (
        "a secrets file's ACL is not read",
        UNIT,
        SECRETS,
        '    let acl = acl_of(&file).map_err(|why| format!("PW_SECRETS_FILE {path}: {why}"))?;',
        "    let acl = None::<Vec<AclEntry>>;\n    let _ = acl_of;",
    ),
    (
        "an ACL's named user is taken as this process's",
        UNIT,
        SECRETS,
        "                        entry.id == uid,",
        "                        true,",
    ),
    (
        "an ACL's entry for anyone is read as nothing",
        UNIT,
        SECRETS,
        "                    ACL_OTHER if entry.perm & (READ | WRITE) != 0 => {",
        "                    ACL_OTHER if false => {",
    ),
    (
        "a secret given twice is taken",
        UNIT,
        SECRETS,
        "                if found.contains_key(known) {",
        "                if found.contains_key(known) && false {",
    ),
    (
        "a line that is not NAME=value is quoted",
        UNIT,
        SECRETS,
        '"{path}, line {at}: not `NAME=value` (the line is not shown: it may hold a \\\n'
        '                         secret)"',
        '"{path}, line {at}: not NAME=value: {line}"',
    ),
    (
        "a secret's Debug shows it",
        UNIT,
        SECRETS,
        '        f.write_str("Secret(..)")',
        '        write!(f, "Secret({})", self.0)',
    ),
    (
        "systemd's credential is not read",
        UNIT,
        SECRETS,
        '    let directory = env("CREDENTIALS_DIRECTORY").filter(|d| !d.is_empty())?;',
        '    let directory = env("PW_NO_SUCH_SETTING").filter(|d| !d.is_empty())?;',
    ),
    (
        "a domain's Debug shows its key",
        UNIT,
        DOMAIN,
        '            .field(\n'
        '                "identity_key",\n'
        "                &if self.identity_key == DEVELOPMENT_KEY {\n"
        '                    "the development key"\n'
        "                } else {\n"
        '                    "a deployment\'s key (not shown)"\n'
        "                },\n"
        "            )",
        '            .field("identity_key", &self.identity_key)',
    ),
    # --- the log -----------------------------------------------------------
    (
        "a health check that passed is said",
        UNIT,
        LOG,
        '    let routine = matches!(route, Some("/stream" | "/_pw/health" | "/_pw/ready"));',
        '    let routine = matches!(route, Some("/stream"));',
    ),
    (
        "a failed routine request is not said",
        UNIT,
        LOG,
        "    !routine || status.is_none_or(|s| s >= 500)",
        "    !routine",
    ),
    (
        "a month is off by one",
        UNIT,
        LOG,
        "    let month = if mp < 10 { mp + 3 } else { mp - 9 };",
        "    let month = if mp < 10 { mp + 3 } else { mp - 8 };",
    ),
    (
        "a request's route is kept as none",
        UNIT,
        LOG,
        "    ASKED.with(|a| a.borrow_mut().route = Some(pattern.chars().take(512).collect()));",
        "    ASKED.with(|a| a.borrow_mut().route = None);\n    let _ = pattern;",
    ),
    # --- the stop ----------------------------------------------------------
    (
        "a drain waits for nothing",
        UNIT,
        SHUTDOWN,
        "        while *open > 0 {",
        "        while *open > 0 && false {",
    ),
    (
        "a closed connection is not counted",
        UNIT,
        SHUTDOWN,
        "        *open -= 1;",
        "        *open -= 0;",
    ),
    (
        "a stop wakes an address nothing listens on",
        UNIT,
        SHUTDOWN,
        "        IpAddr::V4(a) if a.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),",
        "        IpAddr::V4(a) if a.is_unspecified() && false => IpAddr::V4(Ipv4Addr::LOCALHOST),",
    ),
    (
        "a second signal drains again",
        UNIT,
        SHUTDOWN,
        "        (1, None) => Stop::Starting,\n        (1, Some(wake)) => Stop::Drain(wake),",
        "        (_, None) => Stop::Starting,\n        (_, Some(wake)) => Stop::Drain(wake),",
    ),
    # --- no sign-in --------------------------------------------------------
    (
        "a sign-in falls through where there is none",
        UNIT,
        IDENTITY,
        "    fn start(&self, _headers: &str, sign_up: bool) -> Option<Reply> {\n"
        '        let mode = self.mode.read().expect("mode");\n'
        "        if let Mode::Closed = &*mode {",
        "    fn start(&self, _headers: &str, sign_up: bool) -> Option<Reply> {\n"
        '        let mode = self.mode.read().expect("mode");\n'
        "        if let Mode::Closed = &*mode && false {",
    ),
    (
        "with no sign-in every session is signed in",
        UNIT,
        IDENTITY,
        '        *self.mode.write().expect("mode") = Mode::Closed;\n        self.principals.set_accounts(true);',
        '        *self.mode.write().expect("mode") = Mode::Closed;\n        self.principals.set_accounts(false);',
    ),
    # --- kiokun's cache ----------------------------------------------------
    (
        "kiokun's search cache is not where the deployment keeps it",
        UNIT,
        KIOKUN,
        '        Some(dir) => PathBuf::from(dir).join("kiokun-search"),',
        '        Some(_) => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/kiokun-search"),',
    ),
    (
        "kiokun's search index is not opened before its first read",
        UNIT,
        KIOKUN,
        "        self.search.index()?;\n",
        "",
    ),
    # --- every head through production::head --------------------------------
    (
        "an upload's head is written without production::head",
        UNIT,
        UPLOADS,
        "            crate::production::head(self.status),",
        '            "",',
    ),
    # --- the process -------------------------------------------------------
    (
        "a production host's data is not made ready before it listens",
        PROCESS,
        MAIN,
        "        if let Err(e) = server.data.warm() {",
        "        if let Err(e) = Ok::<(), String>(()) {",
    ),
    (
        "a production host sends no HSTS",
        PROCESS,
        MAIN,
        "        production::install_headers(production::HSTS);",
        "        let _ = production::HSTS;",
    ),
    (
        "a production document starts its runtime at no version",
        PROCESS,
        MAIN,
        "                production::install_runtime(own.assets.version());",
        "                let _ = own.assets.version();",
    ),
    (
        "the deployment's key is not installed",
        PROCESS,
        MAIN,
        "    if let Err(e) = production::install_keys(settings.take_keys()) {",
        "    if let Err(e) = Ok::<(), String>(drop(settings.take_keys())) {",
    ),
    (
        "production's resource entries are the development key's",
        PROCESS,
        PRODUCTION,
        "        entry_key(installed())",
        "        entry_key(&Keys::Development)",
    ),
    (
        "a production store with no database starts",
        PROCESS,
        MAIN,
        "        refused.extend(production::build_refusals(&server));\n        if !refused.is_empty() {",
        "        refused.extend(production::build_refusals(&server));\n        if false && !refused.is_empty() {",
    ),
    (
        "a production host's lines are not JSON",
        PROCESS,
        MAIN,
        "        production::log::use_json();",
        "        let _ = production::log::info;",
    ),
    (
        "a stop does not drain",
        PROCESS,
        SHUTDOWN,
        "            GATE.drain();\n            let _ = std::net::TcpStream::connect_timeout",
        "            let _ = &GATE;\n            let _ = std::net::TcpStream::connect_timeout",
    ),
    (
        "an open connection is not waited for",
        PROCESS,
        MAIN,
        "        let spawned = production::connections::spawn(move || {\n"
        "            handle(&server, stream);\n"
        "            drop(open);\n"
        "        });",
        "        drop(open);\n"
        "        let spawned = production::connections::spawn(move || {\n"
        "            handle(&server, stream);\n"
        "        });",
    ),
    (
        "a connection that sends nothing is held for good",
        PROCESS,
        CONNECTIONS,
        "            until: bounds().map(|b| Instant::now() + b.read),",
        "            until: None,",
    ),
    (
        "a request's deadline is each read's (process)",
        PROCESS,
        CONNECTIONS,
        '                .set_read_timeout(Some(left(until, "the request")?))?;',
        "                .set_read_timeout(bounds().map(|b| b.read))?;\n"
        "            let _ = until;",
    ),
    (
        "an answer's deadline is each write's (process)",
        PROCESS,
        CONNECTIONS,
        '                .set_write_timeout(Some(left(until, "the answer")?))?;',
        "                .set_write_timeout(bounds().map(|b| b.write))?;\n"
        "            let _ = until;",
    ),
    (
        "a head that does not arrive in time is answered nothing",
        PROCESS,
        MAIN,
        "            Err(e) if production::connections::timed_out(&e) => return Head::Late,",
        "            Err(e) if production::connections::timed_out(&e) => return Head::Cut,",
    ),
    (
        "the limit of connections is not held",
        PROCESS,
        MAIN,
        "            match production::shutdown::GATE.try_open(settings.max_connections) {",
        "            match production::shutdown::GATE.try_open(usize::MAX) {",
    ),
    (
        "a connection past the limit is dropped unanswered",
        PROCESS,
        MAIN,
        "                    production::connections::turn_away(stream, production::lines());",
        "                    drop(stream);",
    ),
    (
        "a thread the system refuses ends the host",
        PROCESS,
        CONNECTIONS,
        "    std::thread::Builder::new().spawn(run).map(|_| ())",
        "    drop(std::thread::spawn(run));\n    Ok(())",
    ),
    (
        "a host's descriptors are not raised for its connections",
        PROCESS,
        CONNECTIONS,
        "    if enough(limit.rlim_cur) {",
        "    if enough(limit.rlim_cur) || enough(limit.rlim_max) {",
    ),
    (
        "a readiness check waits for its database without bound (process)",
        PROCESS,
        PRODUCTION,
        "                .wait_timeout(checks, left)",
        "                .wait_timeout(checks, Duration::from_secs(3600))",
    ),
    (
        "a stop while the host starts drains instead of ending it",
        PROCESS,
        SHUTDOWN,
        "        (1, None) => Stop::Starting,",
        '        (1, None) => Stop::Drain(([127, 0, 0, 1], 9).into()),',
    ),
    (
        "a second signal drains again (process)",
        PROCESS,
        SHUTDOWN,
        "        _ => Stop::AtOnce,",
        "        (_, Some(wake)) => Stop::Drain(wake),\n        _ => Stop::AtOnce,",
    ),
    (
        "a streaming subscription holds the stop",
        PROCESS,
        MAIN,
        "        if production::shutdown::GATE.draining() {\n"
        '            ended(format_args!("stream ended by a stop at pass {pass}"));',
        "        if false {\n"
        '            ended(format_args!("stream ended by a stop at pass {pass}"));',
    ),
    (
        "a long poll holds the stop",
        PROCESS,
        MAIN,
        "        if production::shutdown::GATE.draining() {\n            break;\n        }\n        let batch = {",
        "        if false {\n            break;\n        }\n        let batch = {",
    ),
    (
        "a request is said by its path",
        PROCESS,
        PRODUCTION,
        "        log::routed(&pattern);",
        "        log::routed(route);\n        let _ = pattern;",
    ),
    (
        "a stopping host keeps taking connections",
        PROCESS,
        MAIN,
        "    drop(listener);\n",
        "    let _ = &listener;\n",
    ),
    (
        "the host listens on the loopback whatever PW_BIND says",
        PROCESS,
        MAIN,
        "    let listener = TcpListener::bind((settings.bind, port))",
        '    let listener = TcpListener::bind(("127.0.0.1", port))',
    ),
    # --- the compiler (PW7001) ----------------------------------------------
    (
        "a page or an upload under a path the platform keeps is taken",
        COMPILER,
        RESERVED,
        "            let Some(reserved) = reserved_by(&path) else {",
        "            let Some(reserved) = reserved_by(&path).filter(|_| false) else {",
    ),
    (
        "a path is reserved because it begins with a reserved one's letters",
        COMPILER,
        RESERVED,
        "            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))",
        "            .is_some()",
    ),
    (
        "a route's query part is read as its path",
        COMPILER,
        RESERVED,
        "    let path = crate::routes::route_parts(path).0;",
        "    let path = path.trim();",
    ),
    (
        "an upload's paths are not read",
        COMPILER,
        RESERVED,
        '            DeclKind::Upload => ["route", "serves"]',
        '            DeclKind::Upload => ["", ""]',
    ),
    # --- the release (deploy/vm/release.sh) --------------------------------
    (
        "a program release.sh does not build is handed on",
        SCRIPT,
        RELEASE,
        "  store | feed | kiokun) ;;\n  *) echo \"release.sh: no program named",
        "  *) ;;\n  x) echo \"release.sh: no program named",
    ),
    (
        "a domain is handed to the VM unchecked",
        SCRIPT,
        RELEASE,
        'if ! [[ "$PLERIS_DOMAIN" =~ ^$LABEL(\\.$LABEL)+$ ]]; then',
        "if false; then",
    ),
    (
        "a destination is handed to ssh unchecked",
        SCRIPT,
        RELEASE,
        'if ! [[ "$PLERIS_SSH" =~ ^[A-Za-z0-9._][A-Za-z0-9._-]*@[A-Za-z0-9.:_-]+$ ]]; then',
        "if false; then",
    ),
    (
        "a key path is handed to ssh and rsync unchecked",
        SCRIPT,
        RELEASE,
        'if ! [[ "$PLERIS_SSH_KEY" =~ ^[A-Za-z0-9._/~+-]+$ ]] || [[ "$PLERIS_SSH_KEY" == -* ]]; then',
        "if false; then",
    ),
    (
        "the copy keeps the builder's uid",
        SCRIPT,
        RELEASE,
        'RSYNC=(rsync -a --chown=root:root --chmod=go-w -e "$RSYNC_SSH")',
        'RSYNC=(rsync -a -e "$RSYNC_SSH")',
    ),
    (
        "the setup's arguments are handed to the VM's shell unquoted",
        SCRIPT,
        RELEASE,
        '  "${SSH[@]}" "bash /root/pleris-setup/setup.sh $(printf \'%q \' "$PLERIS_DOMAIN" "$PROGRAM")"',
        '  "${SSH[@]}" bash /root/pleris-setup/setup.sh "$PLERIS_DOMAIN" "$PROGRAM"',
    ),
]

UNIT_TESTS = [
    PRODUCTION,
    ASSETS,
    CONNECTIONS,
    SECRETS,
    LOG,
    SHUTDOWN,
    SRC / "tests/production_host.rs",
    KIOKUN,
]

# Each suite: its command, and the sources whose assertions may kill.
SUITES = {
    UNIT: (
        ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
         "--", "production"],
        UNIT_TESTS,
    ),
    PROCESS: (
        ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--test", "production_process"],
        [SERVER / "tests/production_process.rs"],
    ),
    COMPILER: (
        ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "reserved_routes"],
        [ROOT / "compiler/pw-core/tests/reserved_routes.rs"],
    ),
    SCRIPT: (["bash", "deploy/vm/release-checks.sh"], []),
}
INVENTORY = {}

# A release check's own failure: `FAIL: <the case>`, one a case.
SCRIPT_FAILURE = re.compile(r"^FAIL: (.+?) \(", re.M)
SCRIPT_PASS = re.compile(r"^   (?:refused|well formed)", re.M)


def run_script(cmd):
    """(valid, passed, failed, the failing checks): the script's own result,
    exit 0 with every case's line, or exit 1 with at least one `FAIL:` of
    its own; bash's own errors, or any other exit, are no result."""
    try:
        # Its standard output and error, one stream, as text (the producer's
        # defaults).
        child = producer_process.run(cmd, cwd=ROOT, timeout=300)
    except subprocess.TimeoutExpired as expired:
        print(expired.output, flush=True)
        return False, 0, 0, ()
    out = child.stdout or ""
    failed = SCRIPT_FAILURE.findall(out)
    passed = len(SCRIPT_PASS.findall(out))
    errs = re.search(r"syntax error|command not found|unbound variable|No such file", out)
    if errs or child.returncode not in (0, 1) or (child.returncode == 1) != bool(failed):
        print(out[-4000:], flush=True)
        return False, 0, 0, ()
    if child.returncode == 0 and "release checks: OK" not in out:
        print(out[-4000:], flush=True)
        return False, 0, 0, ()
    return True, passed, len(failed), tuple(failed)


def run_tests(suite):
    """(valid, passed, failed, the failing tests): a complete named
    inventory, failures only at a test's assertions."""
    cmd, sources = SUITES[suite]
    if suite == SCRIPT:
        return run_script(cmd)
    full = [*cmd, "--format=pretty"] if "--" in cmd else [*cmd, "--", "--format=pretty"]
    try:
        child = producer_process.run(full, cwd=ROOT, timeout=1500)
    except subprocess.TimeoutExpired as expired:
        print(expired.output, flush=True)
        return False, 0, 0, ()
    result = cargo_assertion_results.validate(
        child.returncode, child.stdout, root=ROOT, sources=sources,
        expected=INVENTORY.get(suite))
    if not result.valid:
        print(child.stdout[-4000:], flush=True)
        return False, 0, 0, ()
    INVENTORY.setdefault(suite, cargo_assertion_results.names(child.stdout))
    return True, result.passed, result.failed, result.failure_names


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    watch = mutation_baseline.mutation_bound.start()
    for suite in (UNIT, PROCESS, COMPILER, SCRIPT):
        built, passed, failed, _ = run_tests(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed or watch.stopped:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            built, passed, failed, killers = run_tests(suite)
        if not built or watch.stopped:
            verdict = "REFUSED (invalid native assertion producer)"
            survivors += 1
        elif failed:
            verdict = f"KILLED by {', '.join(killers)} ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)

    counts = {s: sum(1 for m in MUTANTS if m[1] == s) for s in (UNIT, PROCESS, COMPILER, SCRIPT)}
    print(
        f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed "
        f"({counts[UNIT]} unit, {counts[PROCESS]} process, {counts[COMPILER]} compiler, "
        f"{counts[SCRIPT]} release)"
    )
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
