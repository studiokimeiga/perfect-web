#!/usr/bin/env python3
"""Strict ADR-0389 command/receipt controls; each suite is a bounded claim.

Run --list or --check without compiling. Campaigns require a fresh --output
folder; --only selects exact control IDs and reports SUBSET explicitly. Native
inventories are discovered per binary, checked against the source declarations,
then frozen before the baseline. Every selected binary must finish every run,
with no skips and only native assertions at the allowed test sites. PostgreSQL
uses the fixture's owned random-schema receipt and independently checks absence
on both actual databases after the producer is reaped. No destructive cleanup.
Browser/private-media and full-framework acceptance remain separate.
"""
import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src"
MAIN = SERVER / "main.rs"
IDENTITY = SERVER / "identity.rs"
RESOURCE = ROOT / "runtime/pw-resource/src/lib.rs"
MEMORY = SERVER / "tests/session_commands.rs"
POSTGRES = SERVER / "tests/session_commands_pg.rs"
LIB_MODULE = "fallible_command_tests"
SCHEMA_PATTERN = re.compile(r"pw_session_(feed|store)_[0-9a-f]{16}")

# id, scope, description, edits. Multiple edits are one explicitly composite
# control; MUTANTS below exposes every individual anchor to preflight's checker.
CONTROLS = [
    ("render-actor-cache", "memory", "render operation cache ignores actor generation", [
        (MAIN, '''                    Some(render) => render.once(
                        &self
                            .identity
                            .principals()
                            .receipt_reader(session)
                            .to_string(),
                        &name,
                        f,
                    ),
''', '''                    Some(render) => render.once(session, &name, f),
''')]),
    ("memory-held-authority", "memory", "memory command drops its authority hold before publication", [
        (MAIN, '                Some(gate.read().expect("session command authority"))\n',
         '                { let _ = &gate; None::<std::sync::RwLockReadGuard<\'_, ()>> }\n')]),
    ("memory-current-actor", "memory", "local authority predicate accepts changed or expired actors", [
        (IDENTITY, '        if self.is_local() && self.principals.cached_reader(&self.session) == self.reader {\n',
         '        if true || (self.is_local() && self.principals.cached_reader(&self.session) == self.reader) {\n')]),
    ("memory-final-expiry", "memory", "materializer publication omits its final local expiry check", [
        (MAIN, '''                    schedules::write(tx, scheduled)?;
                    if let Some(actor) = actor.filter(|actor| actor.is_local()) {
                        actor.ensure_local_current()?;
                    }
''', '''                    schedules::write(tx, scheduled)?;
                    if let Some(actor) = actor.filter(|actor| actor.is_local()) {
                        let _ = actor;
                    }
''')]),
    ("receipt-generation", "memory", "durable receipt metadata binds arguments but omits reader generation", [
        (MAIN, '        let sent = serde_json::json!({"v":1,"reader":reader,"args":args}).to_string();\n',
         '        let sent = serde_json::Value::Array(args.to_vec()).to_string();\n')]),
    ("document-generation", "memory", "an old document admits a new actor's fresh interaction", [
        (MAIN, '''                if bound.as_ref() == Some(&current)
                    || (bound.is_none() && current == identity::ReaderIdentity::Guest)
''', '''                if true || bound.as_ref() == Some(&current)
                    || (bound.is_none() && current == identity::ReaderIdentity::Guest)
''')]),
    ("receipt-owner-composite", "memory", "COMPOSITE receipt lookup adopts the page owner and skips its reader guard", [
        (MAIN, '                if owner.as_deref().is_some_and(|owner| owner != session) {\n                    return Ok(None);\n                }\n', '                let session = owner.as_deref().unwrap_or(session);\n'),
        (MAIN, '                if bound.as_ref().is_some_and(|reader| reader != &actor.reader) {\n',
         '                if false && bound.as_ref().is_some_and(|reader| reader != &actor.reader) {\n')]),
    ("response-current-authority", "memory", "fixed HTTP command response skips its live delivery decision", [
        (MAIN, '                        if !server.delivery_allowed(&(session.clone(), 0), Some(&reader_identity)) {\n',
         '                        if false && !server.delivery_allowed(&(session.clone(), 0), Some(&reader_identity)) {\n')]),
    ("unknown-cache-rollback", "resource", "resource memoizes an uncertain completion as a successful empty answer", [
        (RESOURCE, '                let value = value.map_err(|_| CommandError::OutcomeUnknown);\n',
         '                let value = Ok(value.unwrap_or_default());\n')]),
    ("unknown-cache-eviction", "resource", "unknown command reservations are evictable and may be reexecuted", [
        (RESOURCE, '            .is_some_and(|c| matches!(c.get(), Some(Ok(_))));\n',
         '            .is_some_and(|c| c.get().is_some());\n')]),
]
for _kind, _counter in [("feed", "|| {}"), ("store", "|| Counted::ran(1)")]:
    _path = SERVER / f"{_kind}_pg.rs"
    CONTROLS.extend([
        (f"{_kind}-transaction-locks", "postgres", f"COMPOSITE {_kind} transaction omits advisory and row authority locks", [
            (_path, f"            crate::hosts::command_session_lock(c, &hash, {_counter})?;\n", ""),
            (_path, f"            crate::hosts::command_session_row(c, &hash, {_counter})?;\n", "")]),
        (f"{_kind}-late-expiry", "postgres", f"{_kind} transaction omits its final DB-clock authority check", [
            (_path, f'''        if let Some(actor) = &self.actor {{
            crate::hosts::command_session_live(c, actor, {_counter})?;
        }}
''', '''        if let Some(actor) = &self.actor {
            let _ = actor;
        }
''')]),
        (f"{_kind}-source-binding", "postgres", f"{_kind} staging accepts another authority adapter", [
            (_path, '        if actor.is_local() || !actor.belongs_to(&self.session_source) {\n',
             '        if false && (actor.is_local() || !actor.belongs_to(&self.session_source)) {\n')]),
    ])
CONTROLS.extend([
    ("postgres-frozen-clock", "postgres", "authority expiry uses transaction start instead of the live database clock", [
        (SERVER / "hosts.rs", '''            "SELECT user_id, handle, name, issuer, epoch FROM sessions \\
         WHERE hash = $1 \\
         AND opened > clock_timestamp() - make_interval(secs => $2) \\
         AND seen > clock_timestamp() - make_interval(secs => $3)",
''', '''            "SELECT user_id, handle, name, issuer, epoch FROM sessions \\
         WHERE hash = $1 \\
         AND opened > transaction_timestamp() - make_interval(secs => $2) \\
         AND seen > transaction_timestamp() - make_interval(secs => $3)",
''')]),
    ("postgres-unlock-proof", "postgres", "a false advisory unlock is accepted as release proof", [
        (SERVER / "hosts.rs", '    if released {\n', '    if true || released {\n')]),
])
CONTROLS.extend([
    ("page-admission-reader", "memory", "bound page replaces its admitted reader with the later mutable cache", [
        (MAIN, '''    query: &str,
    reader_identity: &identity::ReaderIdentity,
) {
    let principals = server.identity.principals();
    principals.with_reader(session, reader_identity, || {
''', '''    query: &str,
    reader_identity: &identity::ReaderIdentity,
) {
    let _admitted = reader_identity;
    let later = server.identity.principals().bound_reader(session);
    let reader_identity = &later;
    let principals = server.identity.principals();
    principals.with_reader(session, reader_identity, || {
''')]),
    ("applied-reader-selection", "memory", "a telling flattens interaction histories of every reader generation", [
        (MAIN, '            let applied = applied.get(&self.identity.principals().receipt_reader(session).to_string()).cloned().unwrap_or_default();\n',
         '            let applied: Vec<String> = applied.values().flatten().cloned().collect();\n')]),
    ("applied-generation-composite", "memory", "COMPOSITE interaction history writes and reads use an empty generation tag", [
        (MAIN, '''                let mine = applied
                    .entry((
                        session.to_string(),
                        self.identity
                            .principals()
                            .receipt_reader(session)
                            .to_string(),
                    ))
''', '''                let mine = applied
                    .entry((session.to_string(), String::new()))
'''),
        (MAIN, '            let applied = applied.get(&self.identity.principals().receipt_reader(session).to_string()).cloned().unwrap_or_default();\n',
         '            let applied = applied.get(&String::new()).cloned().unwrap_or_default();\n')]),
    ("basis-generation-filter", "memory", "committed basis includes another actor generation's speculative versions", [
        (MAIN, '                .filter(|((s, actor, ..), _)| s == session && *actor == reader)\n',
         '                .filter(|((s, actor, ..), _)| s == session && (true || *actor == reader))\n')]),
    ("host-unknown-rollback", "postgres", "an unknown COMMIT acknowledgement becomes a cacheable rollback answer", [
        (MAIN, '                Ok(CommandOutcome::Uncertain(why)) => return Err(why),\n',
         '''                Ok(CommandOutcome::Uncertain(why)) => Answered {
                    committed: false,
                    result: None,
                    why: Some(why),
                    refused: None,
                },
''')]),
])

CONTROLS.extend([
    ("store-unknown-warm-entry", "postgres", "uncertain Store commit leaves its original warm cart materialization fresh", [
        (MAIN, '            self.materializer.invalidate(&self.cart_key(session), 0);\n', ''),
    ]),
])

MUTANTS = [(identifier, description, path, anchor, replacement)
           for identifier, _, description, edits in CONTROLS
           for path, anchor, replacement in edits]

CONTROLS.extend([
    ("private-trace-payload", "memory", "private materializer trace prints bearer cookie and derived query data", [
        (MAIN, '        trace(TraceEvent::Drain {\n            invalidated: invalidated.len(),\n            existing,\n        });\n',
         '        trace(TraceEvent::Drain {\n            invalidated: invalidated.len(),\n            existing,\n        });\n        if std::env::var("PW_TRACE").is_ok() {\n            eprintln!("private drain {session}: {value}");\n        }\n')]),
])

CONTROLS.extend([
    ("private-trace-error", "memory", "private document recovery trace discloses an escaped backend error and cookie", [
        (MAIN, '        trace(TraceEvent::DocumentRecovery);\n',
         '        trace(TraceEvent::DocumentRecovery);\n        if std::env::var("PW_TRACE").is_ok() {\n            eprintln!("private recovery {doc:?}: {_why:?}");\n        }\n')]),
])

LIMITS = [
    "Commands and receipts only; private-media, browser journeys and full framework are separate.",
    "Owner, applied-history tag and both transaction-lock controls are explicitly composite removals, not independent guard proofs.",
    "Cache key generation and owner guards have redundant protections; no individual-kill claim.",
    "The page admission reader is controlled; nested command_actor recapture remains redundant/unproved independently.",
]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git(*arguments):
    return subprocess.check_output(["git", *arguments], cwd=ROOT, text=True).strip()


INPUT_PATHS = ("compiler", "runtime", "packages", "spikes/own-renderer/server", "examples", "scripts",
               "just", "justfile", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml")


def committed_inputs():
    tracked = subprocess.run(["git", "diff", "--quiet", "HEAD", "--", *INPUT_PATHS], cwd=ROOT).returncode
    untracked = git("ls-files", "--others", "--exclude-standard", "-z", "--", *INPUT_PATHS)
    return tracked == 0 and not untracked


def inputs():
    paths = git("ls-files", "-z").split("\0")
    paths += git("ls-files", "--others", "--exclude-standard", "-z").split("\0")
    starts = ("compiler/", "runtime/", "packages/", "spikes/own-renderer/server/", "examples/", "scripts/", "just/")
    paths = {p for p in paths if p and (p.startswith(starts) or p in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "justfile"))}
    return {p: digest(ROOT / p) for p in sorted(paths) if (ROOT / p).is_file()}


def drift(expected):
    try:
        return inputs() != expected
    except OSError:
        return True


def test_module_bounds(source):
    lines = source.read_text().splitlines()
    starts = [i + 1 for i, line in enumerate(lines) if line == f"mod {LIB_MODULE} {{"
              and i and lines[i-1] == "#[cfg(test)]"]
    if len(starts) != 1:
        raise ValueError("resource test boundary is absent or ambiguous")
    start = starts[0]
    end = next((i + 1 for i in range(start, len(lines)) if lines[i] == "}"), None)
    if end is None or any(line.strip() for line in lines[end:]):
        raise ValueError("resource tests are not one recognized EOF module")
    return start, end


def declaration_names(source, prefix):
    text = source.read_text()
    if source == RESOURCE:
        start, end = test_module_bounds(source)
        text = "\n".join(text.splitlines()[start:end-1])
    names = re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(", text)
    cases = re.findall(r"^cases!\s*\{(.*?)^\}", text, re.M | re.S)
    if cases:
        if source != POSTGRES or len(cases) != 1:
            raise ValueError("unrecognized native case inventory")
        entries = re.findall(r"(\w+)\s*,\s*(?:Feed|Store)\s*,\s*\w+\s*;", cases[0])
        remainder = re.sub(r"\w+\s*,\s*(?:Feed|Store)\s*,\s*\w+\s*;", "", cases[0])
        if not entries or remainder.strip():
            raise ValueError("unrecognized PostgreSQL cases macro")
        names += entries
    if not names or len(names) != len(set(names)):
        raise ValueError("empty or duplicate test declarations")
    return tuple(sorted(prefix + name for name in names))


def native_list(output, prefix):
    names = re.findall(r"^(\S+): test$", output, re.M)
    if not names or len(names) != len(set(names)) or any(not name.startswith(prefix) for name in names):
        raise ValueError("empty, duplicate or unexpected binary test inventory")
    return tuple(sorted(names))


def library_sites(output):
    start, end = test_module_bounds(RESOURCE)
    for path, line, _ in re.findall(r"panicked at ([^\n]+?):(\d+):(\d+):", output):
        source = Path(path)
        source = (source if source.is_absolute() else ROOT / source).resolve()
        if source == RESOURCE.resolve() and not start < int(line) < end:
            return False
    return True


def schema_names(text, expected):
    names = text.splitlines()
    if (expected is not None and len(names) != expected) or len(set(names)) != len(names) or any(not SCHEMA_PATTERN.fullmatch(name) for name in names):
        raise ValueError("owned schema receipt is incomplete, duplicate or malformed")
    return {kind: [name for name in names if SCHEMA_PATTERN.fullmatch(name).group(1) == kind]
            for kind in ("feed", "store")}


def process(command, log, *, timeout, env):
    try:
        child = producer_process.run(command, cwd=ROOT, timeout=timeout, env=env)
    except BaseException as error:
        output = getattr(error, "output", "") or ""
        log.write_text(output if isinstance(output, str) else output.decode(errors="replace"))
        if not isinstance(error, (OSError, subprocess.TimeoutExpired)):
            raise
        return {"status": None, "error": type(error).__name__, "log": log.name}, log.read_text()
    log.write_text(child.stdout)
    return {"status": child.returncode, "log": log.name, "log_sha256": digest(log)}, child.stdout


def cleanup(receipt, expected, output, label, env):
    record = {"receipt": receipt.name, "receipt_sha256": digest(receipt), "valid": False}
    try:
        owned = schema_names(receipt.read_text(), None)
    except ValueError as error:
        record["error"] = str(error)
        return record
    record["owned"] = owned
    record["inventory_valid"] = sum(map(len, owned.values())) == expected
    checks = {}
    for kind, names in owned.items():
        if not names:
            continue
        url = env.get(f"PW_{kind.upper()}_DATABASE_URL")
        if not url:
            checks[kind] = {"valid": False, "error": "required database is absent"}
            continue
        values = ",".join("'" + name + "'" for name in names)
        command = ["psql", "--no-psqlrc", "--tuples-only", "--no-align", "--dbname", url,
                   "--command", f"SELECT nspname FROM pg_catalog.pg_namespace WHERE nspname IN ({values})"]
        check, text = process(command, output / f"{label}.cleanup-{kind}.log", timeout=30, env=env)
        check["valid"] = check["status"] == 0 and not text.strip()
        checks[kind] = check
    record["checks"] = checks
    record["valid"] = record["inventory_valid"] and all(check["valid"] for check in checks.values())
    return record


def suites(chosen):
    cargo = ["cargo", "test", "--quiet", "--locked"]
    specs = []
    if chosen in ("memory", "all"):
        specs += [("memory", cargo + ["-p", "pw-dev-server", "--bin", "pw-dev-server"], MEMORY, "tests::session_commands::"),
                  ("resource", cargo + ["-p", "pw-resource", "--lib"], RESOURCE, LIB_MODULE + "::")]
    if chosen in ("postgres", "all"):
        specs += [("postgres", cargo + ["-p", "pw-dev-server", "--bin", "pw-dev-server", "--features", "session-postgres-tests"], POSTGRES, "tests::session_commands_pg::")]
    if chosen == "postgres":
        specs += [("resource", cargo + ["-p", "pw-resource", "--lib"], RESOURCE, LIB_MODULE + "::")]
    return specs


def inventory(specs, output, env, expected_inputs, timeout):
    frozen = []
    for name, cargo, source, prefix in specs:
        command = cargo + ["--", prefix, "--list", "--format=terse"]
        native, text = process(command, output / f"inventory.{name}.log", timeout=timeout, env=env)
        if native["status"] != 0 or drift(expected_inputs):
            raise ValueError(f"{name}: binary inventory build or source stability failed")
        actual = native_list(text, prefix)
        declared = declaration_names(source, prefix)
        if actual != declared:
            raise ValueError(f"{name}: native and declared inventories differ")
        frozen.append({"name": name, "cargo": cargo, "source": str(source.relative_to(ROOT)),
                       "names": actual, "listing": native})
    return frozen


def run_native(frozen, output, label, env, expected_inputs, timeout):
    records = []
    for spec in frozen:
        name = spec["name"]
        command = spec["cargo"] + ["--", *spec["names"], "--exact", "--test-threads=1", "--format=pretty"]
        local = dict(env)
        receipt = output / f"{label}.{name}.schemas.txt"
        if name == "postgres":
            receipt.touch(exist_ok=False)
            local["PW_SESSION_SCHEMA_RECEIPT"] = str(receipt)
        native = text = None
        cleaned = {"valid": name != "postgres"}
        def release_schemas():
            nonlocal cleaned
            if name == "postgres":
                cleaned = cleanup(receipt, len(spec["names"]), output, f"{label}.{name}", local)
        with mutation_source.releasing(release_schemas):
            native, text = process(command, output / f"{label}.{name}.log", timeout=timeout, env=local)
        native["command"] = command
        native["cleanup"] = cleaned
        result = cargo_assertion_results.validate(native["status"], text, root=ROOT,
            sources=[ROOT / spec["source"]], expected=spec["names"])
        sites = name != "resource" or library_sites(text)
        native["native"] = result._asdict()
        native["cleanup_error_marker"] = "SESSION_SCHEMA_CLEANUP_ERROR" in text
        native["valid"] = (result.valid and sites and cleaned["valid"]
                           and not native["cleanup_error_marker"] and not drift(expected_inputs))
        records.append(native)
    return records


def green(records):
    return bool(records) and all(record["valid"] and record["native"]["failed"] == 0 for record in records)


def kill(records):
    return bool(records) and all(record["valid"] for record in records) and any(record["native"]["failed"] > 0 for record in records)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=("memory", "postgres", "all"), default="memory")
    parser.add_argument("--only", action="append", default=[], help="exact control ID; repeatable")
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--check", action="store_true", help="check selected source anchors without compiling")
    parser.add_argument("--baseline-only", action="store_true")
    parser.add_argument("--expected-head")
    parser.add_argument("--output", type=Path, help="new durable output directory; never reused")
    parser.add_argument("--timeout", type=int, default=1200)
    args = parser.parse_args(argv)
    if len({control[0] for control in CONTROLS}) != len(CONTROLS):
        parser.error("duplicate control identities")
    specs = suites(args.suite)
    allowed = {name for name, *_ in specs}
    controls = [control for control in CONTROLS if control[1] in allowed and (not args.only or control[0] in args.only)]
    known = {control[0] for control in controls}
    if not controls or any(identifier not in known for identifier in args.only):
        parser.error("empty/unknown control selection, or control outside selected suite")
    if args.list:
        print(json.dumps([{"id":identifier,"suite":suite,"description":description,"edits":len(edits)}
                          for identifier,suite,description,edits in controls], indent=2))
        return 0
    problems = []
    for identifier, _, _, edits in controls:
        for path, anchor, replacement in edits:
            if anchor == replacement or path.read_text().count(anchor) != 1:
                problems.append(f"{identifier}: anchor is absent, ambiguous or unchanged in {path.relative_to(ROOT)}")
    if problems:
        print("\n".join(problems))
        return 1
    if args.check:
        print(f"anchors valid: {len(controls)} controls, {sum(len(c[3]) for c in controls)} exact edits")
        return 0
    if not args.output or args.timeout <= 0:
        parser.error("campaign requires a new --output directory and positive timeout")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report_path = output / "report.json"
    before = inputs()
    head = git("rev-parse", "HEAD")
    report = {"schema":1,"head":head,"working_tree":git("status", "--porcelain"),
              "producer_sha256":digest(Path(__file__)),"inputs":before,"suite":args.suite,
              "scope":"BASELINE_ONLY" if args.baseline_only else ("FULL" if not args.only else "SUBSET"),
              "selected":[c[0] for c in controls],"coverage_limits":LIMITS,
              "inventories":[],"baseline":[],"controls":[],"restored":False,"valid":False}
    env = dict(os.environ, PW_STORE_TEST_LAYER="memory")
    try:
        if args.expected_head and args.expected_head != head:
            raise ValueError("HEAD does not match --expected-head")
        if args.expected_head and not committed_inputs():
            raise ValueError("exact-head gate refuses tracked or untracked source/recipe changes")
        if "postgres" in allowed and any(not env.get(f"PW_{kind}_DATABASE_URL") for kind in ("FEED", "STORE")):
            raise ValueError("selected PostgreSQL gate requires both explicit database URLs")
        frozen = inventory(specs, output, env, before, args.timeout)
        report["inventories"] = frozen
        report["baseline"] = run_native(frozen, output, "baseline", env, before, args.timeout)
        if not green(report["baseline"]):
            raise ValueError("baseline is not complete and green; no controls credited")
        if not args.baseline_only:
            for identifier, _, description, edits in controls:
                originals = {path: path.read_text() for path, _, _ in edits}
                changed = dict(originals)
                for path, anchor, replacement in edits:
                    changed[path] = changed[path].replace(anchor, replacement, 1)
                expected = dict(before)
                for path, content in changed.items():
                    expected[str(path.relative_to(ROOT))] = hashlib.sha256(content.encode()).hexdigest()
                with ExitStack() as stack:
                    for path, content in changed.items():
                        stack.enter_context(mutation_source.changed(path, originals[path], content))
                    records = run_native(frozen, output, identifier, env, expected, args.timeout)
                restored = not drift(before)
                killed = restored and kill(records)
                report["controls"].append({"id":identifier,"description":description,"runs":records,
                                           "restored":restored,"killed":killed})
                print(f"{identifier}: {'KILLED' if killed else 'INVALID OR SURVIVED'}", flush=True)
                if not killed:
                    raise ValueError(f"{identifier}: control did not earn a complete native assertion kill")
        report["valid"] = True
    except (ValueError, OSError) as error:
        report["error"] = str(error)
        print(f"INVALID: {error}", flush=True)
    except BaseException as error:
        report["abort"] = type(error).__name__
        raise
    finally:
        report["restored"] = not drift(before)
        report["head_unchanged"] = git("rev-parse", "HEAD") == head
        report["valid"] = report["valid"] and report["restored"] and report["head_unchanged"]
        report["artifacts"] = {path.name:digest(path) for path in sorted(output.iterdir())
                               if path.is_file() and path != report_path}
        report_path.write_text(json.dumps(report, indent=2) + "\n")
        print(f"suite={args.suite}, scope={report['scope']}, controls={len(report['controls'])}/{len(controls)}, valid={report['valid']}, restored={report['restored']}", flush=True)
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    sys.exit(main())
