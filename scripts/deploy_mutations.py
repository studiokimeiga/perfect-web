#!/usr/bin/env python3
"""Mutation controls for track deploy (ADR-XXXX, a production host): what
`PW_DEPLOYMENT=production` refuses at start and answers itself.

Each mutant undoes one piece:
- the start: an origin that is not https, or none, taken; no identity key
  required; a key of fewer than 64 digits taken; a test's setting taken; a
  development server listening anywhere; a setting's seconds out of range
  taken; a store, a feed's uploads or kiokun whose data would be the
  process's taken;
- the keys: the development key kept for resource entries, the instance
  tokens keyed with the entries' key or the development one;
- the answers: a test control answered (`/bench/`, E7-P's commands, a menu
  rewritten through the store's page, the whole refusal); a stopping host
  or one whose database does not answer said ready; metrics read with no
  token, by a token's prefix, or by a token of another length; the host's
  own answers or its file refusal skipped; a request's head unbounded;
- the assets: an older build's version, or any address, kept for good;
  gzip sent unasked, or at a weight of zero; a gzip no smaller kept; no
  `Vary`; one tag for both representations; `If-None-Match` not read; a
  version blind to a digest; a member's CRC-32 or length not its bytes';
- the secrets: a file anyone may read, or its group may write, read; a
  secret given twice taken; a line that is not `NAME=value` quoted; a
  secret's `Debug` showing it; systemd's credential not read;
- the log: a routine request said, a failed one not; a month off by one;
- the stop: a drain that waits for nothing, a closed connection not
  counted, the wake sent to an address nothing listens on;
- no sign-in: a sign-in that falls through, every session signed in;
- kiokun's search cache not where the deployment keeps it;
- the process (the binary run as production runs it): no HSTS, the runtime
  started at no version, the deployment's key not installed, lines not
  JSON, a stop that does not drain, an open connection not waited for, no
  read bound, and the listener on the loopback whatever PW_BIND says.

A unit mutant must fail the server's tests of production (`cargo test -p
pw-dev-server --bin pw-dev-server -- production`); a process mutant, the
binary's (`--test production_process`, which runs the store on a database
of its own where PW_STORE_DATABASE_URL names a server). Each kill names the
tests that failed. Only complete native results earn a kill: a build that
fails, a producer that times out, an inventory that differs from the
baseline's, or a failure anywhere but an assertion of a test source is
REFUSED, never a kill.

Run from the repository root; `just e14-deploy` records the output. The
source is restored after every mutant, whatever happens.
"""

import pathlib
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
SECRETS = SRC / "production/secrets.rs"
LOG = SRC / "production/log.rs"
SHUTDOWN = SRC / "production/shutdown.rs"
MAIN = SRC / "main.rs"
IDENTITY = SRC / "identity.rs"
KIOKUN = SRC / "kiokun.rs"

UNIT, PROCESS = "unit", "process"

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
        "no identity key is required",
        UNIT,
        PRODUCTION,
        '        None => refused.push(\n            "a production deployment keys its addresses',
        '        None => drop(\n            "a production deployment keys its addresses',
    ),
    (
        "a key of fewer than 64 digits is taken",
        UNIT,
        PRODUCTION,
        "        let digits = text.len() >= 64",
        "        let digits = text.len() >= 2",
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
        "a setting's seconds out of their range are taken",
        UNIT,
        PRODUCTION,
        "            Ok(seconds) if (low..=high).contains(&seconds) => Duration::from_secs(seconds),",
        "            Ok(seconds) if low <= high => Duration::from_secs(seconds),",
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
    # --- the keys ----------------------------------------------------------
    (
        "resource entries keep the development key",
        UNIT,
        PRODUCTION,
        "        Some(keys) => pw_resource::ExplicitIdentityKey(keys.entries.clone()).identity_key(),",
        "        Some(_) => pw_resource::DevelopmentIdentityKey.identity_key(),",
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
        "    keys.map_or(DEVELOPMENT_INSTANCE_KEY, |k| k.instances.as_str())",
        "    keys.map_or(DEVELOPMENT_INSTANCE_KEY, |_| DEVELOPMENT_INSTANCE_KEY)",
    ),
    # --- the answers -------------------------------------------------------
    (
        "a /bench/ control is answered",
        UNIT,
        PRODUCTION,
        '    route.starts_with("/bench/")\n        || route == "/bench"',
        '    route.starts_with("/bench/x/")\n        || route == "/bench"',
    ),
    (
        "E7-P's menu change is answered",
        UNIT,
        PRODUCTION,
        '\n    "/command/menu_changed",\n',
        "\n",
    ),
    (
        "a menu is rewritten through the store's page",
        UNIT,
        PRODUCTION,
        '|p| p.starts_with("items="))',
        '|p| p.starts_with("items=x"))',
    ),
    (
        "a test's control is answered",
        UNIT,
        PRODUCTION,
        "        if is_test_control(route, query) {",
        "        if is_test_control(route, query) && false {",
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
        "        && production.answer(method, path, &headers, &mut stream)\n",
        "        && false\n        && production.answer(method, path, &headers, &mut stream)\n",
    ),
    (
        "a production host serves its build's other files",
        UNIT,
        MAIN,
        '    if server.production.get().is_some() {\n        respond_build(stream, 404, "text/plain", b"not found");',
        '    if server.production.get().is_some() && false {\n        respond_build(stream, 404, "text/plain", b"not found");',
    ),
    (
        "a request's head is unbounded",
        UNIT,
        MAIN,
        "    if !whole {",
        "    if !whole && false {",
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
        "    if mode & 0o020 != 0 {",
        "    if mode & 0o020 != 0 && false {",
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
    # --- the log -----------------------------------------------------------
    (
        "a health check that passed is said",
        UNIT,
        LOG,
        '    let routine = matches!(route, "/stream" | "/_pw/health" | "/_pw/ready");',
        '    let routine = matches!(route, "/stream");',
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
    # --- the process -------------------------------------------------------
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
        "            Ok(keys) => production::install_keys(keys),",
        "            Ok(keys) => drop(keys),",
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
        MAIN,
        "            production::shutdown::GATE.drain();",
        "            let _ = &production::shutdown::GATE;",
    ),
    (
        "an open connection is not waited for",
        PROCESS,
        MAIN,
        "        let open = production::shutdown::GATE.open();",
        "        let open = ();",
    ),
    (
        "a connection that sends nothing is held for good",
        PROCESS,
        MAIN,
        "            let _ = stream.set_read_timeout(Some(settings.read_timeout));",
        "            let _ = settings.read_timeout;",
    ),
    (
        "the host listens on the loopback whatever PW_BIND says",
        PROCESS,
        MAIN,
        "    let listener = TcpListener::bind((settings.bind, port))",
        '    let listener = TcpListener::bind(("127.0.0.1", port))',
    ),
]

UNIT_TESTS = [
    PRODUCTION,
    ASSETS,
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
}
INVENTORY = {}


def run_tests(suite):
    """(valid, passed, failed, the failing tests): a complete named
    inventory, failures only at a test's assertions."""
    cmd, sources = SUITES[suite]
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
    for suite in (UNIT, PROCESS):
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

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
