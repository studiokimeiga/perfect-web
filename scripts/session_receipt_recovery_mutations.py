#!/usr/bin/env python3
"""Strict bounded receipt representation/read/recovery controls for ADR0389.

Two real native binaries freeze selected source-declared inventories. A control
must finish both inventories, with zero skips and only its named domain
assertion failures. Build/setup/I/O/timeout/cancellation/cleanup failures are
invalid. Each PostgreSQL fixture owns one fresh schema; absence is queried on
both configured databases after native reaping, never repaired by DROP here.
Every mutation restores before continuation; final green baselines and complete
source/head/artifact manifests are required. Older gates supply no head credit.
The lifecycle guards follow upload_authority_mutations at f008e4ba.
"""
import argparse
from contextlib import ExitStack, contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src"
MAIN = SERVER / "main.rs"
MATERIALIZER = ROOT / "runtime/pw-materialize/src/lib.rs"
TESTS = SERVER / "tests/session_receipt_recovery.rs"
PREFIX = "tests::session_receipt_recovery::"
UNIT = "a_receipt_decode_error_is_not_a_missing_committed_interaction"
PG = PREFIX + "a_stale_same_reader_recovers_a_visible_receipt_after_actual_lost_acknowledgement"
SOURCES = [(MATERIALIZER, "tests::"), (TESTS, PREFIX)]
SPECS = [
    {"name":"materializer", "cargo":["cargo","test","--quiet","--locked","-p","pw-materialize","--lib"],
     "source":MATERIALIZER, "prefix":"tests::", "selected":(UNIT,), "schema_count":0},
    {"name":"server", "cargo":["cargo","test","--quiet","--locked","-p","pw-dev-server","--bin","pw-dev-server","--features","session-postgres-tests"],
     "source":TESTS, "prefix":PREFIX, "selected":None, "schema_count":1},
]
SCHEMA_PATTERN = re.compile(r"pw_session_receipt_[0-9a-f]{16}")
INPUT_PATHS = ("compiler", "runtime", "packages", "spikes/own-renderer/server", "examples", "scripts",
               "just", "justfile", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", ".github")

# Each control exports every exact source edit and specific native domain
# oracle. The legacy fallback is intentionally composite; remaining guards are
# removed independently. A false default is masked by browser commit context,
# so the boolean control models an explicit optimistic true fallback.
CONTROLS = [
    ('legacy-decoder-composite', 'COMPOSITE legacy fallback bypasses JSON/object/type/coherence representation validation', [
        (MAIN, '    fn from_kept(kept: &str) -> Result<Answered, String> {\n', '    fn from_kept(kept: &str) -> Result<Answered, String> {\n        // Mutation: legacy permissive fallback bypasses all representation guards.\n        if std::hint::black_box(true) {\n            let v: serde_json::Value = serde_json::from_str(kept).unwrap_or_default();\n            return Ok(Answered {\n                committed: v["committed"] == true,\n                result: v.get("result").cloned(),\n                why: v["why"].as_str().map(str::to_string),\n                refused: v["refused"].as_str().map(str::to_string),\n            });\n        }\n'),
    ], [(PREFIX + 'malformed_persisted_receipts_never_invent_rollback_or_reexecute', 'malformed committed receipt must refuse as unknown, never invent rollback')]),
    ('committed-boolean', 'invalid or missing committed flag is optimistically accepted as success', [
        (MAIN, '        let committed = object\n            .get("committed")\n            .and_then(serde_json::Value::as_bool)\n            .ok_or_else(malformed)?;\n', '        let committed = object\n            .get("committed")\n            .and_then(serde_json::Value::as_bool)\n            .unwrap_or(true);\n'),
    ], [(PREFIX + 'malformed_persisted_receipts_never_invent_rollback_or_reexecute', 'malformed committed receipt must refuse as unknown, never invent rollback')]),
    ('why-type', 'non-string receipt error is silently discarded', [
        (MAIN, '        let why = optional_text("why")?;\n', '        let why = object.get("why").and_then(serde_json::Value::as_str).map(str::to_string);\n'),
    ], [(PREFIX + 'malformed_persisted_receipts_never_invent_rollback_or_reexecute', 'malformed committed receipt must refuse as unknown, never invent rollback')]),
    ('refusal-type', 'non-string receipt refusal is silently discarded', [
        (MAIN, '        let refused = optional_text("refused")?;\n', '        let refused = object.get("refused").and_then(serde_json::Value::as_str).map(str::to_string);\n'),
    ], [(PREFIX + 'malformed_persisted_receipts_never_invent_rollback_or_reexecute', 'malformed committed receipt must refuse as unknown, never invent rollback')]),
    ('committed-error-coherence', 'committed receipt accepts a contradictory error or refusal', [
        (MAIN, '        if committed && (why.is_some() || refused.is_some()) {\n', '        if false && committed && (why.is_some() || refused.is_some()) {\n'),
    ], [(PREFIX + 'malformed_persisted_receipts_never_invent_rollback_or_reexecute', 'malformed committed receipt must refuse as unknown, never invent rollback')]),
    ('browser-durable-commit', 'browser durable receipt accepts a negative outcome that only RAM or Clock may keep', [
        (MAIN, '        if !answer.committed {\n', '        if false && !answer.committed {\n'),
    ], [(PREFIX + 'malformed_persisted_receipts_never_invent_rollback_or_reexecute', 'malformed committed receipt must refuse as unknown, never invent rollback')]),
    ('materializer-read-error', 'materializer receipt read or decode errors become authoritative misses', [
        (MATERIALIZER, '        .map_err(|_| "interaction receipt could not be read".to_string())\n', '        .or(Ok(None))\n'),
    ], [(PREFIX + 'materializer_receipt_read_error_never_reenters_command_evaluation', 'an unreadable committed receipt was treated as a miss and entered command authority'),
        ("tests::" + UNIT, "a committed receipt decode failure was silently reported as a miss")]),
    ('unknown-before-durable', 'retained unknown reservation prevents recovery of a now-visible committed receipt', [
        (MAIN, '                if matches!(self.commands.command_result(&cache_key), Some(Err(_))) {\n                    // A lost acknowledgement may become a known commit once\n                    // its exact durable receipt is visible. Absence/read failure\n                    // still preserves the unknown reservation and never reruns.\n                    return match self.recalled_interaction(&key)? {\n                        Some((first, answer)) if first == sent => {\n                            Answered::from_browser_receipt(&answer).map(Some)\n                        }\n                        _ => Err("interaction outcome could not be confirmed".into()),\n                    };\n                }\n', '                if matches!(self.commands.command_result(&cache_key), Some(Err(_))) {\n                    return Err("interaction outcome could not be confirmed".into());\n                }\n'),
    ], [(PREFIX + 'a_stale_same_reader_recovers_a_visible_receipt_after_actual_lost_acknowledgement', 'a stale same-reader retry must recover a now-visible authoritative receipt before an old unknown reservation')]),
]
MUTANTS = [(identifier, description, path, anchor, replacement)
           for identifier, description, edits, _ in CONTROLS
           for path, anchor, replacement in edits]
LIMITS = [
    "Receipt representation, authoritative read failure and same-reader stale recovery only; no database-at-rest integrity or external exactly-once claim.",
    "The materializer suite selects one exact decode witness; the server suite includes the actual compiled memory and PostgreSQL receipt fixtures and cached-refusal neighbor.",
    "The legacy-fallback control is explicitly composite shape/type/coherence removal; it does not prove each masked representation guard independently.",
    "The real PG lost-ACK seam follows an actual Applied adapter commit; it is not socket loss, OS termination or an unsettled external charge.",
    "Cleanup stdout and native status/reaping prove exact owned-name absence; psql stderr is deliberately discarded at launch and cannot be independently reparsed.",
    "All pre-existing command, scheduler, upload and full-framework gates remain separately required.",
]

class TerminationScope:
    """Unwind TERM through source ownership; defer signals during restoration."""
    def __init__(self):
        self.pending = None
        self.raised = False
        self.restoring = 0
        self.finalizers = []

    def __enter__(self):
        self.previous = signal.getsignal(signal.SIGTERM)
        if self.previous != signal.SIG_IGN:
            signal.signal(signal.SIGTERM, self.cancel)
        return self

    def __exit__(self, *exception):
        signal.signal(signal.SIGTERM, self.previous)

    def cancel(self, number, frame):
        if self.pending is None:
            self.pending = (number, frame)
        if not self.restoring and not self.raised:
            self.deliver()

    def deliver(self):
        self.raised = True
        number, frame = self.pending
        for finalize in reversed(self.finalizers):
            mutation_source.release(finalize, SystemExit(128 + number))
        if callable(self.previous):
            self.previous(number, frame)
        raise SystemExit(128 + number)

    @contextmanager
    def deferred(self, on_cancel=None):
        self.restoring += 1
        if on_cancel is not None:
            self.finalizers.append(on_cancel)
        try:
            yield
        finally:
            try:
                self.restoring -= 1
                if not self.restoring and self.pending is not None and not self.raised:
                    self.deliver()
            finally:
                if on_cancel is not None:
                    self.finalizers.pop()

class RestoringStack(ExitStack):
    def __init__(self, termination):
        super().__init__()
        self.termination = termination

    def __exit__(self, *exception):
        with self.termination.deferred():
            return super().__exit__(*exception)

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def git(*arguments):
    return subprocess.check_output(["git", *arguments], cwd=ROOT, text=True).strip()

def committed_inputs():
    tracked = subprocess.run(["git", "diff", "--quiet", "HEAD", "--", *INPUT_PATHS], cwd=ROOT).returncode
    untracked = git("ls-files", "--others", "--exclude-standard", "-z", "--", *INPUT_PATHS)
    return tracked == 0 and not untracked

def inputs():
    paths = git("ls-files", "-z", "--", *INPUT_PATHS).split("\0")
    paths += git("ls-files", "--others", "--exclude-standard", "-z", "--", *INPUT_PATHS).split("\0")
    return {p: digest(ROOT / p) for p in sorted(set(paths)) if p and (ROOT / p).is_file()}

def drift(expected):
    try:
        return inputs() != expected
    except OSError:
        return True

def declarations(source, prefix, selected=None):
    text = source.read_text()
    names = re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(", text)
    if (not names or len(names) != len(set(names)) or len(names) != text.count("#[test]")
            or re.search(r"^\s*\w+!\s*\{", text, re.M)):
        raise ValueError(f"empty, duplicate or unrecognized test declarations: {source}")
    if selected is not None:
        if not selected or len(selected) != len(set(selected)) or any(name not in names for name in selected):
            raise ValueError("selected source inventory is absent or duplicated")
        names = selected
    return tuple(sorted(prefix + name for name in names))

def native_list(output, prefixes):
    names = re.findall(r"^(\S+): test$", output, re.M)
    if (not names or len(names) != len(set(names))
            or any(not name.startswith(tuple(prefixes)) for name in names)
            or re.search(r"^\S+: (?!test$)[^\n]*$", output, re.M)):
        raise ValueError("empty, duplicate or unexpected binary test inventory")
    return tuple(sorted(names))

def rust_mask(text):
    """Preserve positions while hiding literals/comments for brace matching."""
    tokens = re.compile(r'//[^\n]*|/\*.*?\*/|(?:br|r)(#+)".*?"\1|(?:br|r)"[^"]*"|b?"(?:\\.|[^"\\])*"|b?\'(?:\\.|[^\'\\])\'', re.S)
    return tokens.sub(lambda m: re.sub(r"[^\n]", " ", m.group()), text)

def closed(mask, start, left, right):
    depth = 0
    for index in range(start, len(mask)):
        if mask[index] == left:
            depth += 1
        elif mask[index] == right:
            depth -= 1
            if depth == 0:
                return index
    raise ValueError("unclosed Rust function or assertion")

def witness_sites(specs, witnesses):
    sites = {}
    for witness in witnesses:
        name, message, *helper = witness
        candidates = [(source, prefix) for source, prefix in specs if name.startswith(prefix)]
        if candidates:
            longest = max(len(prefix) for _, prefix in candidates)
            candidates = [(source, prefix) for source, prefix in candidates if len(prefix) == longest]
        if len(candidates) != 1 or name in sites:
            raise ValueError("ambiguous or duplicate witness identity")
        source, prefix = candidates[0]
        text = source.read_text()
        mask = rust_mask(text)
        function = helper[0] if len(helper) == 1 else name[len(prefix):]
        if len(helper) > 1:
            raise ValueError("unrecognized explicit domain-helper witness")
        head = r"fn\s+" if helper else r"#\[test\]\s*fn\s+"
        starts = list(re.finditer(head + re.escape(function) + r"\s*\(", text))
        if len(starts) != 1:
            raise ValueError(f"missing or ambiguous domain witness: {name}: {message}")
        start = mask.find("{", starts[0].end())
        end = closed(mask, start, "{", "}")
        if text[start:end].count(message) != 1:
            raise ValueError(f"missing or ambiguous domain assertion fragment: {name}: {message}")
        at = text.index(message, start, end)
        assertions = []
        for match in re.finditer(r"^[ \t]*assert(?:_eq|_ne)?!\(", text[start:end], re.M):
            assertion = start + match.start()
            opening = mask.find("(", assertion)
            if assertion <= at < closed(mask, opening, "(", ")"):
                line = text.count("\n", 0, assertion) + 1
                column = len(match.group()) - len(match.group().lstrip()) + 1
                assertions.append((str(source.resolve()), line, column))
        if len(assertions) != 1:
            raise ValueError(f"domain witness is not inside one assertion in {name}")
        sites[name] = assertions[0]
    return sites

def failure_sites(output, sites, expected=None):
    output = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", output)
    blocks = re.findall(r"^---- (\S+) stdout ----\n(.*?)(?=^---- |^failures:|^test result:|\Z)", output, re.M | re.S)
    if expected is not None and (len(blocks) != len(expected) or {name for name, _ in blocks} != set(expected)):
        return False
    for name, block in blocks:
        panics = re.findall(r"panicked at ([^\n]+?):(\d+):(\d+):", block)
        if len(panics) != 1 or name not in sites:
            return False
        path, line, column = panics[0]
        path = Path(path)
        path = (path if path.is_absolute() else ROOT / path).resolve()
        if (str(path), int(line), int(column)) != sites[name]:
            return False
    return True

def process(command, log, timeout, env, display_command=None, suppress_stderr=False):
    receipt = log.with_suffix(log.suffix + ".process.json")
    reaped = None
    error = None
    status = None

    def finished(pid, native_status, timed_out, cancelled):
        nonlocal reaped
        reaped = {"pid":pid, "status":native_status, "timed_out":timed_out, "cancelled":cancelled}

    try:
        with log.open("w") as raw:
            child = producer_process.run(command, cwd=ROOT, timeout=timeout, env=env,
                                         stdout=raw, on_reaped=finished,
                                         stderr=subprocess.DEVNULL if suppress_stderr else subprocess.STDOUT)
            status = child.returncode
    except BaseException as caught:
        error = type(caught).__name__
        partial = getattr(caught, "output", "") or ""
        if partial:
            with log.open("a") as raw:
                raw.write(partial if isinstance(partial, str) else partial.decode(errors="replace"))
        if not isinstance(caught, (OSError, subprocess.TimeoutExpired)):
            raise
    finally:
        receipt.write_text(json.dumps({"command":display_command if display_command is not None else command, "error":error, "reaped":reaped,
                                       "stderr_policy":"discarded at child launch" if suppress_stderr else "raw merged stdout/stderr"}, indent=2) + "\n")
    return {"status":status, "error":error, "log":log.name, "log_sha256":digest(log),
            "process_receipt":receipt.name, "process_receipt_sha256":digest(receipt),
            "reaped":bool(reaped and reaped["status"] == status
                          and not reaped["timed_out"] and not reaped["cancelled"])}, log.read_text()

def schema_names(text, expected=None):
    names = text.splitlines()
    if (len(names) != len(set(names)) or any(not SCHEMA_PATTERN.fullmatch(name) for name in names)
            or expected is not None and len(names) != expected):
        raise ValueError("owned receipt schema inventory is missing, duplicate or malformed")
    return names


def cleanup(receipt, expected, output, label, env):
    record = {"receipt":receipt.name, "receipt_sha256":digest(receipt), "valid":False}
    try:
        names = schema_names(receipt.read_text())
    except ValueError as error:
        record["error"] = str(error)
        return record
    record["owned"] = names
    record["inventory_valid"] = len(names) == expected
    checks = {}
    # The same owned names are checked on BOTH configured sources. This is
    # read-only; an interrupted fixture is never repaired by broad deletion.
    # libpq diagnostics can echo password-bearing URI fragments. Discard psql
    # stderr at launch; stdout is only the fixed owned-name SELECT result. A
    # nonzero status remains invalid, with no reconstructed error diagnostics.
    for kind in ("feed", "store"):
        url = env.get(f"PW_{kind.upper()}_DATABASE_URL")
        if not url:
            checks[kind] = {"valid":False, "error":"configured database is absent"}
            continue
        if not names:
            checks[kind] = {"valid":True, "scope":"no owned names recorded"}
            continue
        values = ",".join("'" + name + "'" for name in names)
        query = f"SELECT nspname FROM pg_catalog.pg_namespace WHERE nspname IN ({values})"
        command = ["psql","--no-psqlrc","--tuples-only","--no-align","--dbname",url,"--command",query]
        public_command = command.copy()
        public_command[public_command.index("--dbname") + 1] = f"<configured-{kind}-database>"
        result, text = process(command, output / f"{label}.cleanup-{kind}.log", 30, env,
                               display_command=public_command, suppress_stderr=True)
        result["valid"] = (result["status"] == 0 and result["error"] is None
                           and result["reaped"] and not text.strip())
        checks[kind] = result
    record["checks"] = checks
    record["valid"] = record["inventory_valid"] and len(checks) == 2 and all(c["valid"] for c in checks.values())
    return record


def inventory(output, env, expected, timeout):
    frozen = []
    for spec in SPECS:
        selectors = [spec["prefix"] + name for name in spec["selected"]] if spec["selected"] else [spec["prefix"]]
        command = spec["cargo"] + ["--", *selectors, *(["--exact"] if spec["selected"] else []), "--list", "--format=terse"]
        record, text = process(command, output / f"inventory.{spec['name']}.log", timeout, env)
        if record["status"] != 0 or record["error"] is not None or not record["reaped"] or drift(expected):
            raise ValueError("native inventory build, I/O, reaping or source stability failed")
        actual = native_list(text, (spec["prefix"],))
        declared = declarations(spec["source"], spec["prefix"], spec["selected"])
        if actual != declared:
            raise ValueError("native and source test inventories differ")
        if spec["schema_count"] and (actual.count(PG) != spec["schema_count"]):
            raise ValueError("PostgreSQL fixture count differs from its named inventory")
        frozen.append({"name":spec["name"], "cargo":spec["cargo"], "names":actual,
                       "sources":[str(spec["source"].relative_to(ROOT))], "schema_count":spec["schema_count"],
                       "listing":record, "command":command})
    return frozen


def evaluate(status, text, frozen, expected, sites=None):
    native = cargo_assertion_results.validate(status, text, root=ROOT,
        sources=[ROOT / source for source in frozen["sources"]], expected=frozen["names"])
    domain = sites is None or failure_sites(text, sites, native.failure_names)
    marker = "SESSION_SCHEMA_CLEANUP_ERROR" in text
    return {"native":native._asdict(), "domain_sites_valid":domain, "cleanup_error_marker":marker,
            "valid":native.valid and domain and not marker and not drift(expected)}


def run_native(frozen, output, label, env, expected, timeout, sites=None):
    records = []
    for spec in frozen:
        name = spec["name"]
        command = spec["cargo"] + ["--", *spec["names"], "--exact", "--test-threads=1", "--format=pretty"]
        local = dict(env)
        receipt = output / f"{label}.{name}.schemas.txt"
        if spec["schema_count"]:
            receipt.touch(exist_ok=False)
            local["PW_SESSION_SCHEMA_RECEIPT"] = str(receipt)
        cleaned = {"valid":not spec["schema_count"]}
        def release_schemas():
            nonlocal cleaned
            if spec["schema_count"]:
                cleaned = cleanup(receipt, spec["schema_count"], output, f"{label}.{name}", local)
        with mutation_source.releasing(release_schemas):
            record, text = process(command, output / f"{label}.{name}.log", timeout, local)
        record.update(evaluate(record["status"], text, spec, expected, sites))
        record["cleanup"] = cleaned
        record["valid"] = (record["valid"] and record["reaped"] and record["error"] is None
                           and cleaned["valid"])
        record["command"] = command
        records.append(record)
    return records


def green(records):
    return bool(records) and all(record["valid"] and record["native"]["failed"] == 0 for record in records)


def killed(records):
    return (bool(records) and all(record["valid"] for record in records)
            and any(record["native"]["failed"] > 0 for record in records))


def campaign(argv, termination):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--only", action="append", default=[], help="exact control ID; repeatable")
    parser.add_argument("--baseline-only", action="store_true")
    parser.add_argument("--expected-head")
    parser.add_argument("--output", type=Path, help="new durable directory; never reused")
    parser.add_argument("--timeout", type=int, default=1200)
    args = parser.parse_args(argv)
    identifiers = {control[0] for control in CONTROLS}
    if len(identifiers) != len(CONTROLS) or not identifiers or any(c not in identifiers for c in args.only):
        parser.error("empty, duplicate or unknown control identities")
    controls = [c for c in CONTROLS if not args.only or c[0] in args.only]
    if args.list:
        print(json.dumps([{"id":identifier,"description":description,"edits":len(edits),"witnesses":witnesses}
                          for identifier,description,edits,witnesses in controls], indent=2))
        return 0
    try:
        for identifier, _, edits, witnesses in controls:
            for path, anchor, replacement in edits:
                if anchor == replacement or path.read_text().count(anchor) != 1:
                    raise ValueError(f"{identifier}: absent, ambiguous or unchanged anchor in {path.relative_to(ROOT)}")
            witness_sites(SOURCES, witnesses)
        for spec in SPECS:
            declarations(spec["source"], spec["prefix"], spec["selected"])
    except (ValueError, OSError) as error:
        print(f"INVALID source contract: {error}", flush=True)
        return 1
    if args.check:
        print(f"anchors and domain sites valid: {len(controls)} controls", flush=True)
        return 0
    if not args.output or args.timeout <= 0:
        parser.error("campaign requires a new --output directory and positive timeout")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    before = inputs()
    head = git("rev-parse", "HEAD")
    report = {"schema":1,"head":head,"working_tree":git("status", "--porcelain"),
              "producer_sha256":digest(Path(__file__)),"inputs":before,
              "scope":"BASELINE_ONLY" if args.baseline_only else ("SUBSET" if args.only else "FULL"),
              "coverage_limits":LIMITS,"selected":[c[0] for c in controls],"inventory":None,
              "baseline":None,"controls":[],"final_baseline":None,"restored":False,"valid":False}
    env = dict(os.environ, PW_STORE_TEST_LAYER="memory", CARGO_TERM_COLOR="never", RUST_BACKTRACE="0")
    try:
        if args.expected_head and args.expected_head != head:
            raise ValueError("HEAD does not match --expected-head")
        if args.expected_head and not committed_inputs():
            raise ValueError("exact-head gate requires committed tracked and untracked execution inputs")
        if not env.get("PW_FEED_DATABASE_URL") or not env.get("PW_STORE_DATABASE_URL"):
            raise ValueError("receipt recovery needs both configured PostgreSQL adapter URLs")
        frozen = inventory(output, env, before, args.timeout)
        report["inventory"] = frozen
        report["baseline"] = run_native(frozen, output, "baseline", env, before, args.timeout)
        if not green(report["baseline"]):
            raise ValueError("baseline is not complete and green; no controls credited")
        if not args.baseline_only:
            for identifier, description, edits, witnesses in controls:
                originals = {path:path.read_text() for path, _, _ in edits}
                changed = dict(originals)
                for path, anchor, replacement in edits:
                    if changed[path].count(anchor) != 1:
                        raise ValueError(f"{identifier}: anchor changed after baseline")
                    changed[path] = changed[path].replace(anchor, replacement, 1)
                expected = dict(before)
                for path, text in changed.items():
                    expected[str(path.relative_to(ROOT))] = hashlib.sha256(text.encode()).hexdigest()
                with RestoringStack(termination) as stack:
                    for path, text in changed.items():
                        stack.enter_context(mutation_source.changed(path, originals[path], text))
                    sites = witness_sites(SOURCES, witnesses)
                    run = run_native(frozen, output, identifier, env, expected, args.timeout, sites)
                restored = not drift(before)
                earned = restored and killed(run)
                report["controls"].append({"id":identifier,"description":description,"runs":run,
                                           "domain_sites":sites,"restored":restored,"killed":earned})
                print(f"{identifier}: {'KILLED' if earned else 'INVALID OR SURVIVED'}", flush=True)
                if not earned:
                    raise ValueError(f"{identifier}: complete native domain assertion kill not earned")
        report["final_baseline"] = run_native(frozen, output, "final-baseline", env, before, args.timeout)
        if not green(report["final_baseline"]):
            raise ValueError("final restored baseline is not complete and green")
        report["valid"] = True
    except (ValueError, OSError) as error:
        report["valid"] = False
        report["error"] = str(error)
        print(f"INVALID: {error}", flush=True)
    except BaseException as error:
        report["valid"] = False
        report["abort"] = type(error).__name__
        raise
    finally:
        def write_report():
            (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        def cancelled_report():
            report["valid"] = False
            report["abort"] = "SystemExit"
            write_report()
        with termination.deferred(cancelled_report):
            try:
                report["restored"] = not drift(before)
                report["head_unchanged"] = git("rev-parse", "HEAD") == head
                report["valid"] = (report["valid"] and report["restored"] and report["head_unchanged"]
                                   and termination.pending is None)
                report["artifacts"] = {path.name:digest(path) for path in sorted(output.iterdir()) if path.is_file()}
                if termination.pending is not None:
                    report["abort"] = "SystemExit"
                write_report()
            except BaseException as error:
                report["valid"] = False
                report["abort"] = type(error).__name__
                mutation_source.release(write_report, error)
                raise
            print(f"scope={report['scope']}, controls={len(report['controls'])}/{len(controls)}, valid={report['valid']}, restored={report['restored']}", flush=True)
    return 0 if report["valid"] else 1

def main(argv=None):
    with TerminationScope() as termination:
        return campaign(argv, termination)

if __name__ == "__main__":
    sys.exit(main())
