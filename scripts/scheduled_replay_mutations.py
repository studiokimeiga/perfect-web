#!/usr/bin/env python3
"""ADR0388: whole scheduler replay/atomicity controls with native assertions.

Missing DB, setup/compiler/IO/product panic, worker crash, timeout, changed or
short inventories, or leftover owned schemas are INVALID, never kills.
The cleanup receipt only names freshly created schemas. Native Cargo results
and exact test assertion sites remain the sole pass/kill authority.
"""
import os
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import cargo_assertion_results
import mutation_source
import producer_process

ROOT = Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src"
TESTS = [
 ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server", "--features", "scheduler-postgres-tests", "--", "tests::scheduled_memory::", "tests::scheduled_replay::", "--test-threads=1", "--format=pretty"],
 ["cargo", "test", "--quiet", "--locked", "-p", "pw-materialize", "--lib", "scheduled::tests::", "--", "--format=pretty"],
]
SOURCES = [SERVER / "tests/scheduled_memory.rs", SERVER / "tests/scheduled_replay.rs", ROOT / "runtime/pw-materialize/src/scheduled.rs"]
INVENTORY = {}
# (fault, source, exact anchor, replacement)
MUTANTS = [
    ('foreign queue adapters share authority', ROOT / 'runtime/pw-materialize/src/scheduled.rs', '        Arc::ptr_eq(&self.0, &other.0)\n', '        let _ = other; true\n'),
    ('pure Clock success has no platform commit', ROOT / 'spikes/own-renderer/server/src/main.rs', '            None if !scheduled.is_empty() || completing.is_some() => Vec::new(),\n', '            None if !scheduled.is_empty() => Vec::new(),\n'),
    ('memory completion skips transactional consumption', ROOT / 'spikes/own-renderer/server/src/main.rs', '                        if !Materializer::consume_run(tx, completion.run, completion.now_ms)? {\n', '                        if false && !Materializer::consume_run(tx, completion.run, completion.now_ms)? {\n'),
    ('store completion skips its authoritative fence', ROOT / 'spikes/own-renderer/server/src/store_pg.rs', '        if let Some(completion) = completing {\n', '        if let Some(completion) = completing.filter(|_| false) {\n'),
    ('feed completion skips its authoritative fence', ROOT / 'spikes/own-renderer/server/src/feed_pg.rs', '        if let Some(completion) = completing {\n', '        if let Some(completion) = completing.filter(|_| false) {\n'),
    ('a changed PG generation is claimed consumed', ROOT / 'spikes/own-renderer/server/src/schedules.rs', '            return Ok(Fence::NotCurrent);\n', '            return Ok(Fence::Consumed);\n'),
    ('a suppressed PG completion delete counts as consumption', ROOT / 'spikes/own-renderer/server/src/schedules.rs', '            Fence::Refused\n', '            Fence::Consumed\n'),
    ('a suppressed memory completion delete counts as consumption', ROOT / 'runtime/pw-materialize/src/scheduled.rs', '        if changed != 1 {\n', '        if false && changed != 1 {\n'),
    ('zero-row retry persistence is called stale without recheck', ROOT / 'spikes/own-renderer/server/src/main.rs', '            Ok(false) => self.scheduled_update_blocked(source, run),\n            Err(why) => {\n                eprintln!("pw dev server: scheduled retry was not kept: {why}");', '            Ok(false) => false,\n            Err(why) => {\n                eprintln!("pw dev server: scheduled retry was not kept: {why}");'),
    ('driver ignores the requested blocked wait', ROOT / 'spikes/own-renderer/server/src/main.rs', '            wait(delay);\n', '            wait(1.min(delay));\n'),
    ('legacy work executes without reconciliation', ROOT / 'spikes/own-renderer/server/src/main.rs', '        if run.protocol != 1 {\n', '        if false && run.protocol != 1 {\n'),
    ('Clock uses ordinary completion semantics', ROOT / 'spikes/own-renderer/server/src/main.rs', '            Some(run.completing(self.now())),\n', '            Some(run.completing(self.now())).filter(|_| false),\n'),
    ('scoped job locks retain every completed generation', ROOT / 'spikes/own-renderer/server/src/main.rs', '        held.retain(|_, weak| weak.strong_count() != 0);\n', ''),
    ('Clock retains per-session presentation rows', ROOT / 'spikes/own-renderer/server/src/main.rs', '            rows.clear();\n', '            rows.truncate(rows.len());\n'),
    ('Clock regenerates synthetic private presentation state', ROOT / 'spikes/own-renderer/server/src/main.rs', '                if completing.is_none() {\n                    self.drain_held(session);\n', '                if true {\n                    self.drain_held(session);\n'),
    ('Clock enters retained client applied history', ROOT / 'spikes/own-renderer/server/src/main.rs', '            if let Some(interaction) = interaction.filter(|_| completing.is_none()) {\n', '            if let Some(interaction) = interaction {\n'),
    ('declared errors commit attempted business effects', ROOT / 'spikes/own-renderer/server/src/main.rs', '            if let [Val::Result(Err(_))] = out.as_slice() {\n', '            if false && let [Val::Result(Err(_))] = out.as_slice() {\n'),
    ('unsigned receipt instants wrap through signed SQL time', ROOT / 'runtime/pw-materialize/src/lib.rs', 'i64::try_from(at_ms).unwrap_or(i64::MAX)', 'at_ms as i64'),
]

def cleanup_verified(receipt, url, required):
    schemas = receipt.read_text().splitlines()
    if ((required and not schemas) or len(set(schemas)) != len(schemas)
            or any(not re.fullmatch(r"pw_scheduler_(?:store|feed)_[0-9a-f]{16}", s) for s in schemas)):
        return False
    if not schemas:
        return True
    values = ",".join("'" + s + "'" for s in schemas)
    try:
        child = producer_process.run(["psql", "--no-psqlrc", "--tuples-only", "--no-align", "--dbname", url,
            "--command", f"SELECT nspname FROM pg_namespace WHERE nspname IN ({values})"],
            cwd=ROOT, timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return False
    # No destructive recovery: an owned leftover is invalid evidence to inspect.
    valid = child.returncode == 0 and not child.stdout.strip()
    if not valid:
        remaining = [name for name in child.stdout.splitlines() if name in schemas]
        print("OWNED_SCHEMA_CLEANUP_STATUS " + json.dumps({"status": child.returncode, "remaining": remaining}), flush=True)
    return valid


def assertions_are_in_tests(output):
    """Mixed runtime source is allowed only after its explicit test boundary."""
    mixed = ROOT / "runtime/pw-materialize/src/scheduled.rs"
    sites = []
    for path, line, _ in re.findall(r"panicked at ([^\n]+?):(\d+):(\d+):", output):
        source = Path(path)
        source = (source if source.is_absolute() else ROOT / source).resolve()
        if source == mixed.resolve():
            sites.append(int(line))
    if not sites:
        return True
    try:
        lines = mixed.read_text().splitlines()
        boundaries = [i + 1 for i, line in enumerate(lines)
                      if line == "mod tests {" and i and lines[i-1] == "#[cfg(test)]"]
        if len(boundaries) != 1:
            return False
        boundary = boundaries[0]
        # rustfmt's top-level test module is the file's final item. Refuse an
        # unrecognized tail instead of guessing a Rust parser around strings.
        end = next(i + 1 for i in range(boundary, len(lines)) if lines[i] == "}")
        if any(line.strip() for line in lines[end:]):
            return False
    except (OSError, StopIteration):
        return False
    return all(boundary < line <= end for line in sites)


def report_abort(receipt, error):
    # The child has been reaped by producer_process; preserve owned names in
    # the durable campaign log before TemporaryDirectory removes its receipt.
    try:
        names = receipt.read_text().splitlines()
    except OSError:
        names = []
    owned = [name for name in names if re.fullmatch(r"pw_scheduler_(?:store|feed)_[0-9a-f]{16}", name)]
    print("OWNED_SCHEDULER_SCHEMAS_ON_ABORT " + json.dumps(owned), flush=True)
    output = getattr(error, "output", None)
    if isinstance(output, str):
        print(output[-5000:], flush=True)


def run_tests():
    url = os.environ.get("PW_STORE_DATABASE_URL") or os.environ.get("PW_FEED_DATABASE_URL")
    if not url:
        print("INVALID: selected scheduler gate requires PostgreSQL", flush=True)
        return False, 0, 0, ()
    env = dict(os.environ, PW_STORE_DATABASE_URL=url, PW_FEED_DATABASE_URL=url, PW_STORE_TEST_LAYER="memory")
    passed = failed = 0
    killers = []
    for number, command in enumerate(TESTS):
        with tempfile.TemporaryDirectory(prefix="pw-scheduler-producer-") as directory:
            receipt = Path(directory) / "owned-schemas.txt"
            receipt.touch()
            env["PW_SCHEDULER_SCHEMA_RECEIPT"] = str(receipt)
            try:
                child = producer_process.run(command, cwd=ROOT, env=env, timeout=1200)
            except BaseException as error:
                mutation_source.release(lambda: report_abort(receipt, error), error)
                if not isinstance(error, (OSError, subprocess.TimeoutExpired)):
                    raise
                print("INVALID: native producer did not complete", flush=True)
                return False, passed, failed, ()
            cleaned = cleanup_verified(receipt, url, required=number == 0)
            result = cargo_assertion_results.validate(child.returncode, child.stdout,
                root=ROOT, sources=SOURCES, expected=INVENTORY.get(tuple(command)))
            test_sites = not result.failed or assertions_are_in_tests(child.stdout)
            if not cleaned or not result.valid or not test_sites:
                if not cleaned:
                    report_abort(receipt, RuntimeError("schema cleanup was not established"))
                print(child.stdout[-5000:], flush=True)
                print(f"INVALID: native inventory/assertion/cleanup (cleanup={cleaned})", flush=True)
                return False, passed, failed, ()
            INVENTORY.setdefault(tuple(command), cargo_assertion_results.names(child.stdout))
            passed += result.passed
            failed += result.failed
            killers.extend(result.failure_names)
    return True, passed, failed, tuple(killers)


def main():
    import mutation_baseline
    watch = mutation_baseline.mutation_bound.start()
    valid, passed, failed, _ = run_tests()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not valid or not passed or failed or watch.stopped:
        print("FAIL: invalid or red unmutated baseline", flush=True)
        mutation_baseline.explain()
        return 1
    chosen = [m for m in MUTANTS if not sys.argv[1:] or any(x in m[0] for x in sys.argv[1:])]
    print(f"campaign scope: {'FULL' if len(chosen)==len(MUTANTS) else 'SUBSET'} {len(chosen)}/{len(MUTANTS)} controls", flush=True)
    if not chosen:
        return 1
    killed = 0
    for what, path, anchor, replacement in chosen:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: INVALID anchor", flush=True)
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            valid, passed, failed, names = run_tests()
        if not valid or watch.stopped:
            verdict = "INVALID (native producer/inventory/assertion/cleanup)"
        elif failed:
            killed += 1
            verdict = "KILLED by " + ", ".join(names) + f" ({failed}/{passed+failed} native assertions)"
        else:
            verdict = "SURVIVED"
        print(f"{what}: {verdict}", flush=True)
    print(f"{killed} of {len(chosen)} mutants killed", flush=True)
    return 0 if killed == len(chosen) else 1


if __name__ == "__main__":
    sys.exit(main())
