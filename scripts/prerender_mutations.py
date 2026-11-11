#!/usr/bin/env python3
"""ADR-0409: a page no reader decides is one document.

Each control undoes one rule:
- of the mark (`compiler/pw-core/src/prerender.rs`): an address, no value from
  it, no query, no stream, a public scope, no handler that sends a command, in
  a block's arms or a view that contains itself; the build that runs the mark;
  a plan that says nothing of a page not marked;
- of the host (`spikes/own-renderer/server/src/`): only a marked page rendered
  once; one that does not render stopping the host; the arm that answers one,
  and its place after the host's own routes; every request still admitted; how
  it is kept, compressed and validated; each representation's own validator;
  a 304 that says what a cache needs and no length; what every document says of
  itself; the gzip trailer.

What the browser does with a page the back-forward cache brings back is
`scripts/device_store_mutations.py`'s, in Chromium.

Strict, as the other controls of this kind: the shared native assertion
validator (each failing test fails at one `assert` in the named sources), the
exact test inventory on the baseline and every control, every source restored
and checked by digest. A build, timeout or other panic is an invalid run, never
a kill; the first invalid or surviving control stops it. Run from the
repository root, whole.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = Path(__file__).resolve().parent.parent
RULE = ROOT / "compiler/pw-core/src/prerender.rs"
BUILD = ROOT / "compiler/pw-core/src/build.rs"
PLAN = ROOT / "compiler/pw-core/src/page_values.rs"
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
SERVER = ROOT / "spikes/own-renderer/server/src"
MAIN = SERVER / "main.rs"
HOST = SERVER / "prerender.rs"
HOST_TESTS = SERVER / "tests/prerender.rs"
TODOMVC_TESTS = SERVER / "tests/todomvc.rs"
RULE_TESTS = ROOT / "compiler/pw-core/tests/prerender.rs"

RULE_NAMES = [
    "a_handler_that_sends_a_command_keeps_a_page_unmarked",
    "a_handler_that_stays_in_the_browser_leaves_a_page_marked",
    "a_layout_is_read_for_its_handlers",
    "a_layout_that_binds_a_query_keeps_a_page_unmarked",
    "a_layout_with_a_stream_keeps_a_page_unmarked",
    "a_page_its_address_gives_a_value_is_not_marked",
    "a_page_marked_is_said_so_in_its_plan",
    "a_page_of_signals_at_an_address_is_marked",
    "a_page_that_binds_a_query_is_not_marked",
    "a_page_that_is_someones_is_not_marked",
    "a_page_with_a_stream_is_not_marked",
    "a_page_with_no_address_is_not_marked",
    "a_predicate_asked_by_a_layout_or_a_view_keeps_a_page_unmarked",
    "a_value_the_device_keeps_leaves_a_page_marked",
    "a_view_that_contains_itself_is_read_for_its_handlers",
]
HOST_NAMES = [
    "tests::a_compiled_module_is_sent_as_its_packaging_wrote_it",
    "tests::prerender::a_browser_that_holds_a_representation_is_told_so",
    "tests::prerender::a_gzip_trailers_checksum_is_crc_32",
    "tests::prerender::a_head_is_answered_as_its_get_is",
    "tests::prerender::a_host_route_keeps_its_place",
    "tests::prerender::a_page_someone_decides_is_as_it_was",
    "tests::prerender::a_page_that_does_not_render_stops_the_host",
    "tests::prerender::a_page_with_no_rows_is_the_same_bytes_as_a_request_would_have_been",
    "tests::prerender::a_request_with_a_session_is_admitted_as_every_request_is",
    "tests::prerender::a_versioned_build_says_its_delivery_and_names_its_files_at_it",
    "tests::prerender::each_representation_holds_the_document",
    "tests::prerender::every_reader_is_sent_the_same_document_and_named_nothing",
    "tests::prerender::it_is_rendered_in_the_public_partition",
    "tests::prerender::it_is_the_document_a_request_would_have_been_rendered",
    "tests::prerender::it_says_what_a_document_says_of_itself",
    "tests::prerender::nothing_is_one_document_until_it_is_rendered",
    "tests::prerender::the_address_that_still_renders_is_as_it_was",
    "tests::prerender::a_validator_is_the_first_128_bits_of_its_digest",
    "tests::prerender::what_it_says_of_itself_is_part_of_its_validator",
    "tests::prerender::todomvc_is_one_document",
]
SUITES = {
    "rule": {
        "command": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "prerender",
                    "--", *RULE_NAMES, "--exact", "--test-threads=1", "--format=pretty"],
        "names": RULE_NAMES,
        "sources": [RULE_TESTS],
    },
    "host": {
        "command": ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin",
                    "pw-dev-server", "--", *HOST_NAMES, "--exact", "--test-threads=1",
                    "--format=pretty"],
        "names": HOST_NAMES,
        # The files a failing test may fail in: its assertion's own.
        "sources": [HOST_TESTS, TODOMVC_TESTS, MAIN],
    },
}

# (what, file, anchor, replacement). The suite is the file's.
MUTANTS = [
    # --- the mark -----------------------------------------------------------
    ("a page with no address is marked", RULE,
     '    plan.route.is_some()\n        && plan.params.is_empty()\n',
     '    plan.params.is_empty()\n'),
    ("a page its address gives a value is marked", RULE,
     '        && plan.params.is_empty()\n        && plan.bindings.is_empty()\n',
     '        && plan.bindings.is_empty()\n'),
    ("a page that binds a query is marked", RULE,
     '        && plan.bindings.is_empty()\n        && plan.streams.is_empty()\n',
     '        && plan.streams.is_empty()\n'),
    ("a page with a stream is marked", RULE,
     '        && plan.streams.is_empty()\n        && plan.predicates.is_empty()\n',
     '        && plan.predicates.is_empty()\n'),
    ("a page that asks its reader a predicate is marked", RULE,
     '        && plan.predicates.is_empty()\n        && plan.scope == "public"\n',
     '        && plan.scope == "public"\n'),
    ("a view's predicate is a name no document holds", TEMPLATE,
     '    let mut names = asked_predicates(ctx.hirs, ctx.ws, ctx.sigs, def.unit);\n',
     '    let mut names = BTreeMap::new();\n'),
    ("a page that is someone's is marked", RULE,
     '        && plan.scope == "public"\n',
     '\n'),
    ("a handler's commands are not read", RULE,
     '                .is_some_and(|sent| sent.is_empty()),\n',
     '                .is_some(),\n'),
    ("a block's arms are not read", RULE,
     '        here && part.nested().into_iter().all(|region| self.chunks(region))\n',
     '        here\n'),
    ("a view that contains itself is not read", RULE,
     '            Part::Instance { path, .. } => self.view(path),\n',
     '            Part::Instance { .. } => true,\n'),
    ("the build does not mark a page", BUILD,
     '    crate::prerender::mark(&mut pages, &templates, &handlers);\n',
     ''),
    ("a page not marked is said so in its plan", PLAN,
     '    #[serde(default, skip_serializing_if = "std::ops::Not::not")]\n    pub prerender: bool,\n',
     '    #[serde(default)]\n    pub prerender: bool,\n'),
    # --- the host -----------------------------------------------------------
    ("a page not marked is rendered once too", MAIN,
     '            if plan["prerender"] != true {\n                continue;\n            }\n',
     ''),
    ("it is rendered in a session's partition", MAIN,
     '                .render_page_in(page, &BTreeMap::new(), Partition::Public, &[])\n',
     '                .render_page_in(page, &BTreeMap::new(), session_partition("once"), &[])\n'),
    ("a page that does not render is passed over", MAIN,
     '            let (document, _, _) = self\n'
     '                .render_page_in(page, &BTreeMap::new(), Partition::Public, &[])\n'
     '                .map_err(|why| format!("`{page}` cannot be prerendered: {why}"))?;\n',
     '            let Ok((document, _, _)) =\n'
     '                self.render_page_in(page, &BTreeMap::new(), Partition::Public, &[])\n'
     '            else {\n'
     '                continue;\n'
     '            };\n'),
    ("a marked page is rendered for each request", MAIN,
     '        ("GET", _) if prerendered.is_some() => {\n',
     '        ("GET", _) if false && prerendered.is_some() => {\n'),
    ("a marked page answers before the host's own routes", MAIN,
     '    match (method, route) {\n'
     '        // **A command the compiler built, by its component id**: what a\n',
     '    if method == "GET"\n'
     '        && let Some(document) = prerendered\n'
     '    {\n'
     '        respond_prerendered(&mut stream, &headers, document);\n'
     '        return;\n'
     '    }\n'
     '    match (method, route) {\n'
     '        // **A command the compiler built, by its component id**: what a\n'),
    ("a marked page's request is not admitted", MAIN,
     '        && ((method == "POST" && path.starts_with("/command/")) || route == "/stream")\n',
     '        && ((method == "POST" && path.starts_with("/command/"))\n'
     '            || route == "/stream"\n'
     '            || (method == "GET" && route == "/" && server.prerendered.get().is_some()))\n'),
    ("it is kept as a session's answer", MAIN,
     '    const POLICY: &str = "public, no-cache";\n',
     '    const POLICY: &str = "private, no-store";\n'),
    ("it names a session", MAIN,
     '            "cache-control: {POLICY}\\r\\netag: {}\\r\\nvary: accept-encoding\\r\\n{encoding}{}",\n',
     '            "set-cookie: pw-session=x\\r\\ncache-control: {POLICY}\\r\\netag: {}\\r\\nvary: accept-encoding\\r\\n{encoding}{}",\n'),
    ("it says nothing of what it is encoded in", MAIN,
     '        .map_or(String::new(), |e| format!("content-encoding: {e}\\r\\n"));\n',
     '        .map_or(String::new(), |_| String::new());\n'),
    ("it does not say what a document says of itself", MAIN,
     '            chosen.tag, document.headers\n',
     '            chosen.tag, ""\n'),
    ("a request that holds it is sent it again", MAIN,
     '    if holds_representation(request_headers, &chosen.tag) {\n',
     '    if false && holds_representation(request_headers, &chosen.tag) {\n'),
    ("a weak tag does not hold it", MAIN,
     '            one.strip_prefix("W/").unwrap_or(one) == tag\n',
     '            one == tag\n'),
    ("`*` holds nothing", MAIN,
     '        held.trim() == "*"\n            || held.split(\',\').any(|one| {\n',
     '        held.split(\',\').any(|one| {\n'),
    ("only the first tag listed is read", MAIN,
     '            || held.split(\',\').any(|one| {\n',
     '            || held.split(\',\').take(1).any(|one| {\n'),
    ("a 304 states a length", MAIN,
     '        "HTTP/1.1 304 {}\\r\\ncache-control:',
     '        "HTTP/1.1 304 {}\\r\\ncontent-length: 0\\r\\ncache-control:'),
    ("a 304 does not say what it varies by", MAIN,
     '\\r\\netag: {tag}\\r\\nvary: accept-encoding\\r\\n{NOSNIFF}{}\\r\\n",\n',
     '\\r\\netag: {tag}\\r\\n{NOSNIFF}{}\\r\\n",\n'),
    ("a request that holds a build's file is sent it again", MAIN,
     '    if holds_representation(request_headers, &tag) {\n',
     '    if false && holds_representation(request_headers, &tag) {\n'),
    ("it does not say its build", MAIN,
     '                prerender::Prerendered::of(document, format!("{build}{delivery}{policy}"))\n',
     '                prerender::Prerendered::of(document, format!("{delivery}{policy}"))\n'),
    ("it does not say its delivery", MAIN,
     '                prerender::Prerendered::of(document, format!("{build}{delivery}{policy}"))\n',
     '                prerender::Prerendered::of(document, format!("{build}{policy}"))\n'),
    ("every representation has one validator", HOST,
     '        let tag = strong_tag(&[bytes.as_slice(), says.as_bytes()].concat());\n',
     '        let tag = strong_tag(says.as_bytes());\n'),
    ("a validator does not name what the document says of itself", HOST,
     '        let tag = strong_tag(&[bytes.as_slice(), says.as_bytes()].concat());\n',
     '        let tag = strong_tag(&bytes);\n'),
    ("a validator is its whole digest", HOST,
     '        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..16])\n',
     '        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..])\n'),
    ("Brotli is not offered", HOST,
     '        if accepts("br") {\n',
     '        if false && accepts("br") {\n'),
    ("gzip is not offered", HOST,
     '        } else if accepts("gzip") {\n',
     '        } else if false && accepts("gzip") {\n'),
    ("the gzip checksum is another polynomial's", HOST,
     '            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());\n',
     '            crc = (crc >> 1) ^ (0xEDB8_8321 & (crc & 1).wrapping_neg());\n'),
    ("the gzip length is big-endian", HOST,
     '    out.extend((text.len() as u32).to_le_bytes());\n',
     '    out.extend((text.len() as u32).to_be_bytes());\n'),
]


def suite_of(path):
    return "rule" if path in (RULE, BUILD, PLAN, TEMPLATE) else "host"


# How long one run of the tests may take. Past it the run is invalid.
TIMEOUT = 1200


def run_tests(name):
    suite = SUITES[name]
    try:
        child = producer_process.run(suite["command"], cwd=ROOT, timeout=TIMEOUT)
    except (OSError, subprocess.TimeoutExpired) as error:
        print(f"INVALID test producer: {type(error).__name__}", flush=True)
        return None
    print(child.stdout, flush=True)
    return cargo_assertion_results.validate(child.returncode, child.stdout, root=ROOT,
                                            sources=suite["sources"], expected=suite["names"])


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    files = sorted({path for _, path, _, _ in MUTANTS}
                   | {path for suite in SUITES.values() for path in suite["sources"]})
    before = {str(path.relative_to(ROOT)): digest(path) for path in files}
    report = {"scope": "a page no reader decides is one document (ADR-0409)",
              "commands": {name: suite["command"] for name, suite in SUITES.items()},
              "producer_sha256": digest(Path(__file__)),
              "sources": before, "baseline": {}, "controls": [], "restored": False,
              "valid": False}
    try:
        for name, suite in SUITES.items():
            baseline = run_tests(name)
            report["baseline"][name] = baseline._asdict() if baseline else None
            if (not baseline or not baseline.valid or baseline.failed
                    or baseline.passed != len(suite["names"])):
                print(f"the {name} suite's baseline:")
                print("FAIL: the unmutated baseline is not green; no control can mean anything")
                mutation_baseline.explain()
                return 1
        for name, path, anchor, replacement in MUTANTS:
            original = path.read_text()
            if original.count(anchor) != 1 or anchor == replacement:
                print(f"INVALID anchor: {name}")
                report["error"] = f"anchor: {name}"
                return 1
            with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
                result = run_tests(suite_of(path))
            restored = digest(path) == before[str(path.relative_to(ROOT))]
            killed = bool(restored and result and result.valid and result.failed > 0)
            report["controls"].append({"name": name, "killed": killed, "restored": restored,
                                       "native": result._asdict() if result else None})
            print(f"{name}: {'KILLED' if killed else 'INVALID OR SURVIVED'}"
                  + (f" ({result.failed} of {result.passed + result.failed} tests fail)"
                     if killed else ""), flush=True)
            if not killed:
                return 1
        report["valid"] = True
    finally:
        report["restored"] = all(digest(path) == before[str(path.relative_to(ROOT))]
                                 for path in files)
        report["valid"] = report["valid"] and report["restored"]
        if args.report:
            args.report.write_text(json.dumps(report, indent=2) + "\n")
        killed = sum(control["killed"] for control in report["controls"])
        print(f"{killed} of {len(MUTANTS)} mutants killed; valid={report['valid']}, "
              f"restored={report['restored']}", flush=True)
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    sys.exit(main())
