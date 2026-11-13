#!/usr/bin/env python3
"""Mutation controls for ADR-0229: a computed condition decides its block.

Each mutant undoes one piece: a subject read by the path the compiler names,
with what it reads; the browser's subject, its block rendered again as the
signal changes, and its arms holding no view's signals; the host's subject,
its block among those it renders again; a value the host computes inside a
block, and a browser value inside a host-rendered frame, refused; the block a computed subject
decides as one a signal does; the host's first value of a subject; and the
browser's renderer given the values computed now, and setting no block in
place.

The compiler's and the server's tests must fail for a mutant of the compiler
or the server ("cargo"). A mutant of the runtime must fail
`e2e/feed.spec.mjs` in three engines, against a build of the mutated source
("browser").

Run from the repository root; `just e14-computed-conditions` records the
output. The source is restored after every mutant, whatever happens.
"""

import os
import json
import secrets
import tempfile
import pathlib
import signal
import subprocess
import sys

import browser_test_results
import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
VALUES = ROOT / "compiler/pw-core/src/page_values.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what is undone, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a computed subject reads nothing",
        "cargo",
        TEMPLATE,
        "    let inputs = inputs_of(body, e, ctx);\n",
        "    let inputs = Vec::new();\n",
    ),
    (
        "a computed subject is read as a path",
        "cargo",
        TEMPLATE,
        "        read.inputs = Some(inputs);\n    }\n    value\n",
        "        let _ = (read, inputs);\n    }\n    value\n",
    ),
    (
        "a subject the browser computes decides no block",
        "cargo",
        VALUES,
        # Re-anchored by track `device-store`: a subject's inputs, one or
        # several.
        "                    browser_subjects.insert(read.part.0, (inputs, derived.component_id.clone()));\n",
        "                    let _ = inputs;\n",
    ),
    (
        "a block whose subject the browser computes is not rendered again for its signal",
        "cargo",
        VALUES,
        # block_reads already supplies these inputs since ADR-0366. Remove
        # the final subject subscriptions, not its now-redundant fallback.
        "                reads,\n                attribute: String::new(),\n"
        "                owns: owned(entry.id.0),\n                derived: derived.clone(),\n",
        "                reads: reads.into_iter().filter(|s| !inputs.iter().any(|(input, _)| input == s)).collect(),\n"
        "                attribute: String::new(),\n"
        "                owns: owned(entry.id.0),\n                derived: derived.clone(),\n",
    ),
    (
        "a block whose subject the browser computes may hold a view's signals",
        "cargo",
        VALUES,
        "            if !owned(entry.id.0).is_empty() {\n",
        "            if false {\n",
    ),
    (
        "a block a host's subject decides is not rendered again",
        "cargo",
        VALUES,
        "                || derived_values.iter().any(|d| d.path == entry.value)\n                || part.nested()",
        "                || false\n                || part.nested()",
    ),
    (
        "a value the host computes in a block is a text part of its own",
        "cargo",
        VALUES,
        "                Computes::Host(part) if hole.nested => derived_values.push(part),\n",
        "",
    ),
    (
        "a browser-computed value inside a host-rendered block is built",
        "cargo",
        VALUES,
        # Browser-rendered nested values are supported by ADR-0366. The
        # unsupported boundary is a host-rendered frame, checked here.
        "                        (_, Reach::Frame) if lifted.is_some() => {\n"
        "                            return Err(format!(\n"
        "                                \"part {id} computes a value from {named} inside a \\\n"
        "                                 block a value other than a signal decides, which the browser \\\n"
        "                                 does not render again (ruling 0073-a)\"\n"
        "                            ));\n                        }\n",
        "                        (_, Reach::Frame) if lifted.is_some() => {}\n",
    ),
    (
        "a block a computed subject decides is not one a signal does",
        "cargo",
        VALUES,
        "                    Reach::Top if signal(value) || browser.contains(value) => {\n"
        "                        Reach::Live(Vec::new())\n",
        # signal(value) includes the canonical computed mapping now; the
        # old removal of browser.contains alone is equivalent.
        "                    Reach::Top if signals.contains(&root(value)) => {\n"
        "                        Reach::Live(Vec::new())\n",
    ),
    (
        "the host renders no first value of a subject",
        "cargo",
        SERVER,
        "                    | pw_render::ir::Part::Conditional { value, .. }\n",
        "",
    ),
    (
        "the browser renders a block without what it computes",
        "browser",
        RUNTIME,
        "  const values = renderer.env([...signals, ...computed]);\n",
        "  const values = renderer.env([...signals]);\n",
    ),
    (
        "the browser sets a block in place",
        "browser",
        RUNTIME,
        "    if (live.derived && SET_IN_PLACE.has(live.kind)) {\n",
        "    if (live.derived) {\n",
    ),
]

# Separate binaries ensure a failure never prevents later original tests from running.
CARGO = {
    "core": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core",
             "--test", "computed_conditions", "--", "--format=pretty"],
    "holes": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core",
             "--test", "computed_holes", "--", "--format=pretty"],
    "signals": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core",
             "--test", "computed_signals", "--", "--format=pretty"],
    "templates": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core",
             "--test", "template_values", "--", "--format=pretty"],
    "host": ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server",
             "--bin", "pw-dev-server",
             "--", "a_condition_", "from_a_signal", "--format=pretty"],
}
NAMES = {
    "core": {
        "a_computed_condition_is_a_hosts_from_a_query_and_the_browsers_from_a_signal",
        "an_else_if_a_match_and_a_value_in_a_hosts_block_are_computed_too",
        "what_a_condition_does_not_compute_yet_is_refused_by_name",
    },
    "holes": {
        "a_build_input_a_template_reads_is_no_hosts",
        "a_computed_attribute_is_a_value_the_host_sets",
        "a_member_an_imported_module_lacks_does_not_resolve",
        "a_value_the_template_computes_is_lifted_and_planned",
        "a_value_the_template_computes_performs_nothing",
        "a_view_composed_in_the_page_computes_from_the_pages_value",
        "a_view_that_contains_itself_computes_nothing_yet_and_a_speculated_value_is_computed",
        "what_a_host_does_not_compute_is_refused_by_name",
    },
    "signals": {
        "a_signal_the_wire_holds_otherwise_is_decoded_for_its_function",
        "a_value_computed_from_a_signal_is_the_browsers_and_its_first_the_hosts",
        "an_attribute_and_a_views_field_of_a_signal_are_computed_as_well",
        "standalone_and_prepared_signal_modules_are_byte_identical",
        "what_the_browser_does_not_compute_yet_is_refused_by_name",
    },
    "templates": {
        "a_computed_hole_checks_and_does_not_build",
        "a_directive_other_than_on_does_not_build",
        "a_key_is_the_path_from_the_element",
        "a_loops_key_is_read_from_its_element",
        "the_ir_blocks_a_computed_list_and_a_foreign_key",
    },
    "host": {
        "tests::a_condition_a_host_computes_decides_its_block_and_again_when_it_changes",
        "tests::a_condition_the_browser_computes_is_rendered_at_its_first_value",
        "tests::what_a_page_computes_from_a_signal_is_rendered_at_its_first_value",
    },
}
SOURCES = {
    'core': ROOT / 'compiler/pw-core/tests/computed_conditions.rs',
    'holes': ROOT / 'compiler/pw-core/tests/computed_holes.rs',
    'signals': ROOT / 'compiler/pw-core/tests/computed_signals.rs',
    'templates': ROOT / 'compiler/pw-core/tests/template_values.rs',
    "host": SERVER,
}
SPEC = ["e2e/feed.spec.mjs"]
BOUND = 1200
INVENTORIES = {}


def bounded(cmd, **kw):
    """Shared native process ownership; timeout is invalid, never a test failure."""
    try:
        result = producer_process.run(cmd, timeout=BOUND, **kw)
        return result.stdout, result.returncode
    except subprocess.TimeoutExpired as expired:
        return expired.output, None


def cargo_result(suite, code, output):
    result = cargo_assertion_results.validate(code, output, root=ROOT,
        sources=[SOURCES[suite]], expected=NAMES[suite])
    return (True, result.passed, result.failed) if result.valid else (False, 0, 0)


def cargo_tests():
    valid, passed, failed = True, 0, 0
    for suite, cmd in CARGO.items():
        out, code = bounded(cmd, cwd=ROOT)
        ok, p, f = cargo_result(suite, code, out)
        if not ok or f:
            print(out, flush=True)
        valid &= ok
        passed += p
        failed += f
    return valid, passed, failed


def build():
    """Build the actual source; refusal cannot earn a mutation kill."""
    for command, env in (
        (["bash", "spikes/own-renderer/run.sh"], {"BUILD_ONLY": "1"}),
        (["bash", "spikes/own-renderer/feed.sh"], {}),
        (["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], {}),
    ):
        out, code = bounded(command, cwd=ROOT, env={**os.environ, **env})
        if code != 0:
            print(out, flush=True)
            return False
    return True


def browser_tests():
    if not build():
        return False, 0, 0
    with tempfile.TemporaryDirectory(prefix="pw-computed-conditions-") as directory:
        receipt = pathlib.Path(directory) / "result.json"
        nonce = secrets.token_hex(32)
        out, code = bounded(
            ["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--",
             "pnpm", "exec", "playwright", "test", *SPEC, "--retries=0",
             "--reporter=line,./assertion-result-reporter.mjs"],
            cwd=ROOT / "spikes/own-renderer",
            env={**os.environ, "PW_ASSERTION_REPORT": str(receipt), "PW_ASSERTION_NONCE": nonce},
        )
        try:
            report = json.loads(receipt.read_text())
        except (OSError, ValueError):
            report = None
        result = browser_test_results.parse(code, report, nonce, INVENTORIES.get("browser"))
        if not result.valid or result.failed:
            print(out, flush=True)
        if not result.valid:
            print(f"REFUSED browser producer: {result.reason}", flush=True)
            return False, 0, 0
        INVENTORIES.setdefault("browser", result.inventory)
        return True, result.passed, result.failed


SUITES = {"cargo": cargo_tests, "browser": browser_tests}


def main():
    import mutation_baseline  # Begin native output capture before the baseline.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    watch = mutation_baseline.mutation_bound.start()
    INVENTORIES.clear()
    for suite in sorted({m[1] for m in MUTANTS}):
        valid, passed, failed = SUITES[suite]()
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not valid or failed or not passed or watch.stopped:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    killed = refused = survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: REFUSED (anchor not unique in {path.name})")
            refused += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            valid, passed, failed = SUITES[suite]()
        if not valid or watch.stopped:
            verdict = "REFUSED (invalid or stopped producer; no kill)"
            refused += 1
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
            killed += 1
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)
        if watch.stopped:
            print("REFUSED: memory guard stopped the campaign")
            return 1
    if not build() or watch.stopped:
        print("REFUSED: restored source did not build")
        refused += 1
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived")
    return int(bool(refused or survivors))


if __name__ == "__main__":
    sys.exit(main())
