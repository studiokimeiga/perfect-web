#!/usr/bin/env python3
"""Strict, bounded upload-session authority controls for ADR0389.

--list and --check never compile. Native campaigns freeze the actual test
inventory against explicit source declarations, require every test with zero
skips, and credit only a control's named privacy assertion sites. Setup, build,
I/O, timeout and cleanup failures are invalid, never kills. Each source edit is
restored after its owned child is reaped; a final green baseline is mandatory.
The full source manifest, exact head, raw logs and process receipts are retained.
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
UPLOADS = SERVER / "uploads.rs"
MAIN = SERVER / "main.rs"
SOURCES = [
    (SERVER / "uploads/session_authority_tests.rs", "uploads::tests::session_authority_tests::"),
    (SERVER / "tests/session_upload_authority.rs", "tests::session_upload_authority::"),
    (SERVER / "tests/session_upload_reserved_routes.rs", "tests::session_upload_reserved_routes::"),
]
CARGO = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server"]

# id, description, exact source edits, (full native test identity, unique
# domain assertion fragment). All anchors and witnesses are checked before a
# campaign. No assertion in a setup helper is an accepted kill site.
PREFIX = "uploads::tests::session_authority_tests::"
HTTP_PREFIX = "tests::session_upload_authority::"
CONTROLS = [
    ("http-admitted-reader", "upload dispatch drops the actor admitted before its suspension", [
        (MAIN, '''        server.identity.principals().with_reader(
            &session,
            request_reader.as_ref().expect("upload request admission"),
            || {
                server.uploads.answer(
                    method,
                    route,
                    &headers,
                    &session,
                    fresh,
                    &mut reader,
                    &mut stream,
                )
            },
        );
''', '''        server.uploads.answer(
            method, route, &headers, &session, fresh, &mut reader, &mut stream,
        );
''')], [(HTTP_PREFIX + "upload_http_dispatch_keeps_admitted_reader_when_same_cookie_is_reopened",
         "HTTP upload dispatch must not recapture a replacement generation")]),
    ("preview-lease-reader", "private preview accepts another invocation reader's lease", [
        (UPLOADS, '                .filter(|l| l.id == id && l.reader == reader)\n',
         '                .filter(|l| l.id == id)\n')], [
        (PREFIX + "stale_bound_reader_cannot_take_replacement_generations_lease",
         "stale request cannot read replacement's image"),
        (HTTP_PREFIX + "upload_http_dispatch_keeps_admitted_reader_when_same_cookie_is_reopened",
         "HTTP upload dispatch must not recapture a replacement generation")]),
    ("shown-lease-reader", "attachment metadata accepts another generation's lease", [
        (UPLOADS, '            .filter(|lease| lease.reader == reader)?;\n',
         '            .filter(|_| true)?;\n')], [
        (PREFIX + "completed_logout_refuses_old_cookie_preview_shown_and_claim",
         "logout must hide the attachment metadata"),
        (PREFIX + "same_cookie_same_account_new_generation_cannot_preview_or_claim_old_lease",
         "a new generation cannot inherit metadata"),
        (PREFIX + "stale_bound_reader_cannot_take_replacement_generations_lease",
         "bound reader is not recaptured from the live cache")]),
    ("claim-lease-reader", "a command can acquire a predecessor generation's private lease", [
        (UPLOADS, '            .filter(|l| !l.claimed && l.reader == reader)?;\n',
         '            .filter(|l| !l.claimed)?;\n')], [
        (PREFIX + "completed_logout_refuses_old_cookie_preview_shown_and_claim",
         "logout must not acquire a predecessor's lease"),
        (PREFIX + "same_cookie_same_account_new_generation_cannot_preview_or_claim_old_lease",
         "a new generation cannot acquire the old image"),
        (PREFIX + "stale_bound_reader_cannot_take_replacement_generations_lease",
         "stale command cannot claim replacement's image")]),
    ("preview-live-inspection", "matching frozen preview bytes skip live authority inspection", [
        (UPLOADS, '            Ok(Some(bytes)) if self.reader_current(session, &reader) => {\n',
         '            Ok(Some(bytes)) => {\n')], [
        (PREFIX + "original_bound_reader_cannot_preview_after_completed_logout",
         "preview must inspect live authority even when the lease matches its original bound reader"),
        (PREFIX + "unavailable_source_cannot_serve_preview_from_cached_bound_reader",
         "unavailable source must refuse preview instead of borrowing cached authority"),
        (PREFIX + "remote_revocation_without_local_refresh_cannot_serve_preview",
         "source revocation must hide preview even before this host refreshes"),
        (PREFIX + "preview_live_inspection_follows_the_complete_staged_blob_read",
         "preview must revalidate authority after staged storage finishes reading private bytes")]),
    ("post-body-current-composite", "COMPOSITE publication omits all three post-body current-authority checks", [
        (UPLOADS, '''            .map(|gate| gate.read().expect("session authority gate"));
        if !self.reader_current(session, reader) {
''', '''            .map(|gate| gate.read().expect("session authority gate"));
        if false && !self.reader_current(session, reader) {
'''),
        (UPLOADS, '''        // command retains the existing transaction-bound commit fence.
        if !self.reader_current(session, reader) {
''', '''        // command retains the existing transaction-bound commit fence.
        if false && !self.reader_current(session, reader) {
'''),
        (UPLOADS, '        if local && !self.reader_current(session, reader) {\n',
         '        if false && local && !self.reader_current(session, reader) {\n')], [
        (PREFIX + "upload_held_in_body_cannot_publish_a_lease_after_completed_logout",
         "an upload cannot publish after its reader was revoked"),
        (PREFIX + "remote_revocation_while_upload_body_is_held_refuses_staging",
         "post-body source change must refuse lease publication despite a matching local cache",
         "a_source_change_in_body_refuses_staging"),
        (PREFIX + "source_failure_while_upload_body_is_held_refuses_staging",
         "post-body source change must refuse lease publication despite a matching local cache",
         "a_source_change_in_body_refuses_staging"),
        (PREFIX + "local_expiry_after_staged_put_refuses_publication_and_releases_gate",
         "authority expiry during storage must refuse publication"),
        (PREFIX + "source_failure_after_staged_put_refuses_new_lease_and_removes_unheld_bytes",
         "source failure during storage must refuse publication"),
        (PREFIX + "source_failure_after_same_key_put_preserves_existing_lease_and_shared_bytes",
         "failed same-key replacement must not publish"),
        (PREFIX + "a_lease_claimed_during_final_inspection_is_not_replaced",
         "an incoming upload cannot replace a lease claimed during source inspection"),
        (PREFIX + "a_lease_replaced_during_final_inspection_is_not_overwritten",
         "an incoming upload cannot overwrite a different lease installed during source inspection")]),
    ("post-storage-source-inspection", "prepared lease skips its independent current-source inspection outside the state mutex", [
        (UPLOADS, '''        // command retains the existing transaction-bound commit fence.
        if !self.reader_current(session, reader) {
''', '''        // command retains the existing transaction-bound commit fence.
        if false && !self.reader_current(session, reader) {
''')], [
        (PREFIX + "source_failure_after_staged_put_refuses_new_lease_and_removes_unheld_bytes",
         "source failure during storage must refuse publication"),
        (PREFIX + "source_failure_after_same_key_put_preserves_existing_lease_and_shared_bytes",
         "failed same-key replacement must not publish"),
        (PREFIX + "a_lease_claimed_during_final_inspection_is_not_replaced",
         "an incoming upload cannot replace a lease claimed during source inspection"),
        (PREFIX + "a_lease_replaced_during_final_inspection_is_not_overwritten",
         "an incoming upload cannot overwrite a different lease installed during source inspection")]),
    ("preview-inspection-order", "preview authority is checked before the complete private blob is read", [
        (UPLOADS, '''        match self.staged.get(&key) {
            // Freeze all bytes before current inspection; an old binding
''', '''        let prematurely_current = self.reader_current(session, &reader);
        match self.staged.get(&key) {
            // Freeze all bytes before current inspection; an old binding
'''),
        (UPLOADS, '            Ok(Some(bytes)) if self.reader_current(session, &reader) => {\n',
         '            Ok(Some(bytes)) if prematurely_current => {\n')], [
        (PREFIX + "preview_live_inspection_follows_the_complete_staged_blob_read",
         "preview must revalidate authority after staged storage finishes reading private bytes")]),
    ("local-publication-gate", "staged storage runs without its local revocation read guard", [
        (UPLOADS, '            .map(|gate| gate.read().expect("session authority gate"));\n',
         '            .map(|gate| { let _ = gate; });\n')], [
        (PREFIX + "local_expiry_after_staged_put_refuses_publication_and_releases_gate",
         "local publication must hold the authority read gate through staged storage")]),
    ("previous-lease-current-composite", "COMPOSITE publication ignores both the exact prepared predecessor and a concurrent claim", [
        (UPLOADS, '''        if state.leases.get(session).map(|lease| &lease.id) != previous.as_ref()
            || state.leases.get(session).is_some_and(|lease| lease.claimed)
        {
''', '''        if false && (state.leases.get(session).map(|lease| &lease.id) != previous.as_ref()
            || state.leases.get(session).is_some_and(|lease| lease.claimed))
        {
''')], [
        (PREFIX + "a_lease_claimed_during_final_inspection_is_not_replaced",
         "an incoming upload cannot replace a lease claimed during source inspection"),
        (PREFIX + "a_lease_replaced_during_final_inspection_is_not_overwritten",
         "an incoming upload cannot overwrite a different lease installed during source inspection")]),
    ("provisional-refusal-cleanup", "a refused prepared lease retains its provisional hold and capacity charge", [
        (UPLOADS, '''impl Drop for PendingLease<'_> {
    fn drop(&mut self) {
        if self.held {
''', '''impl Drop for PendingLease<'_> {
    fn drop(&mut self) {
        if false && self.held {
''')], [
        (PREFIX + "local_expiry_after_staged_put_refuses_publication_and_releases_gate", "state.staged_bytes"),
        (PREFIX + "source_failure_after_staged_put_refuses_new_lease_and_removes_unheld_bytes", "state.staged_bytes"),
        (PREFIX + "source_failure_after_same_key_put_preserves_existing_lease_and_shared_bytes", "state.staged_bytes"),
        (PREFIX + "a_lease_claimed_during_final_inspection_is_not_replaced",
         "refused reservation releases only its byte charge"),
        (PREFIX + "a_lease_replaced_during_final_inspection_is_not_overwritten", "state.staged_bytes")]),
]
MUTANTS = [(identifier, description, path, anchor, replacement)
           for identifier, description, edits, _ in CONTROLS
           for path, anchor, replacement in edits]
LIMITS = [
    "Temporary upload leases, their private previews and HTTP admission only; public committed images retain their declared policy.",
    "Native fixtures exercise memory authority and the existing Kept inspection seam; no real PostgreSQL or browser execution is claimed here.",
    "The existing whole uploads gate remains separate; this producer credits only the listed authority controls and domain assertions.",
    "The post-body control removes all three current checks together; the local check under the final mutex is redundant in the current expiry witness and is not independently credited.",
    "The independent post-storage source control uses Kept callbacks; the local expiry check remains intact. Predecessor ID and concurrent-claim removals are one explicitly composite control.",
    "The gate witness observes a real held read guard during staged put; it does not claim a remote database transaction fence for ephemeral lease publication.",
    "Reserved command/stream upload routes run in every native baseline/control inventory; their native panic regression is not advertised as an assertion-only mutation kill.",
]
INPUT_PATHS = ("compiler", "runtime", "packages", "spikes/own-renderer/server", "examples", "scripts",
               "just", "justfile", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", ".github")


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


def declarations(source, prefix):
    text = source.read_text()
    names = re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(", text)
    if (not names or len(names) != len(set(names)) or len(names) != text.count("#[test]")
            or re.search(r"^\s*\w+!\s*\{", text, re.M)):
        raise ValueError(f"empty, duplicate or unrecognized test declarations: {source}")
    return tuple(sorted(prefix + name for name in names))


def native_list(output, prefixes):
    names = re.findall(r"^(\S+): test$", output, re.M)
    if (not names or len(names) != len(set(names))
            or any(not name.startswith(tuple(prefixes)) for name in names)
            or re.search(r"^\S+: (?:test\b(?!$)|benchmark\b).+$", output, re.M)):
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


def process(command, log, timeout, env):
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
                                         stdout=raw, on_reaped=finished)
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
        receipt.write_text(json.dumps({"command":command, "error":error, "reaped":reaped}, indent=2) + "\n")
    return {"status":status, "error":error, "log":log.name, "log_sha256":digest(log),
            "process_receipt":receipt.name, "process_receipt_sha256":digest(receipt),
            "reaped":bool(reaped and reaped["status"] == status
                          and not reaped["timed_out"] and not reaped["cancelled"])}, log.read_text()


def inventory(output, env, expected, timeout):
    prefixes = tuple(prefix for _, prefix in SOURCES)
    command = CARGO + ["--", *prefixes, "--list", "--format=terse"]
    record, text = process(command, output / "inventory.log", timeout, env)
    if record["status"] != 0 or not record["reaped"] or drift(expected):
        raise ValueError("native inventory build, reaping or source stability failed")
    actual = native_list(text, prefixes)
    declared = tuple(sorted(name for source, prefix in SOURCES for name in declarations(source, prefix)))
    if actual != declared:
        raise ValueError("native and source test inventories differ")
    return {"names":actual, "sources":[str(path.relative_to(ROOT)) for path, _ in SOURCES],
            "listing":record, "command":command}


def evaluate(status, text, frozen, expected, sites=None):
    result = cargo_assertion_results.validate(status, text, root=ROOT,
        sources=[ROOT / source for source in frozen["sources"]], expected=frozen["names"])
    domain = sites is None or failure_sites(text, sites, result.failure_names)
    cleanup_error = "UPLOAD_AUTHORITY_CLEANUP_ERROR" in text or "SESSION_SCHEMA_CLEANUP_ERROR" in text
    return {"native":result._asdict(), "domain_sites_valid":domain, "cleanup_error_marker":cleanup_error,
            "valid":result.valid and domain and not cleanup_error and not drift(expected)}


def run_native(frozen, output, label, env, expected, timeout, sites=None):
    command = CARGO + ["--", *frozen["names"], "--exact", "--test-threads=1", "--format=pretty"]
    record, text = process(command, output / f"{label}.log", timeout, env)
    record.update(evaluate(record["status"], text, frozen, expected, sites))
    record["valid"] = record["valid"] and record["reaped"]
    record["command"] = command
    return record


def green(record):
    return bool(record) and record["valid"] and record["native"]["failed"] == 0


def killed(record):
    return bool(record) and record["valid"] and record["native"]["failed"] > 0


def main(argv=None):
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
        for source, prefix in SOURCES:
            declarations(source, prefix)
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
                with ExitStack() as stack:
                    for path, text in changed.items():
                        stack.enter_context(mutation_source.changed(path, originals[path], text))
                    sites = witness_sites(SOURCES, witnesses)
                    run = run_native(frozen, output, identifier, env, expected, args.timeout, sites)
                restored = not drift(before)
                earned = restored and killed(run)
                report["controls"].append({"id":identifier,"description":description,"run":run,
                                           "domain_sites":sites,"restored":restored,"killed":earned})
                print(f"{identifier}: {'KILLED' if earned else 'INVALID OR SURVIVED'}", flush=True)
                if not earned:
                    raise ValueError(f"{identifier}: complete native domain assertion kill not earned")
        report["final_baseline"] = run_native(frozen, output, "final-baseline", env, before, args.timeout)
        if not green(report["final_baseline"]):
            raise ValueError("final restored baseline is not complete and green")
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
        report["artifacts"] = {path.name:digest(path) for path in sorted(output.iterdir()) if path.is_file()}
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"scope={report['scope']}, controls={len(report['controls'])}/{len(controls)}, valid={report['valid']}, restored={report['restored']}", flush=True)
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    sys.exit(main())
