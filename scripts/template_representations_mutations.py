#!/usr/bin/env python3
"""Mutation controls for ADR-0232: an opaque value's representation is read
in a template.

Each mutant undoes one piece: a `.value` of an opaque value read by the
opaque value's own path; a record's field named `value` kept a field; a
base the checker's types leave untyped typed by the value relations; and the
speculation module comparing a view's path with the template's as the
template reads it.

The compiler's and the server's tests must fail for each ("cargo").

Run from the repository root; `just e14-representations` records the
output. The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import signal
import subprocess
import sys
import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
SPECULATION = ROOT / "compiler/pw-core/src/backend/speculation.rs"

# (what is undone, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a representation is read as a field",
        "cargo",
        TEMPLATE,
        "        Expr::Field { base, name } if name == \"value\" && representation(*base) => {\n",
        "        Expr::Field { base, name } if name == \"value\" && representation(*base) && false => {\n",
    ),
    (
        "a record's field named `value` is a representation",
        "cargo",
        TEMPLATE,
        "        .is_some_and(|t| t.representation.is_some())\n",
        "        .is_some_and(|_| true)\n",
    ),
    (
        "a base the checker's types leave untyped is typed no further",
        "cargo",
        TEMPLATE,
        "            crate::values::Ty::Nominal(d, _) => Some(d),\n",
        "            crate::values::Ty::Nominal(_, _) => None,\n",
    ),
    (
        "the speculation module reads a representation as a field",
        "cargo",
        SPECULATION,
        "        let Some(own) = crate::template_ir::value_path_typed(\n"
        "            (cx.sigs, cx.ws),\n"
        "            (cx.hirs, at, decl),\n"
        "            written,\n"
        "            hole.expr,\n"
        "        ) else {\n",
        "        let Some(own) = crate::template_ir::value_path_of(written, hole.expr) else {\n",
    ),
]

CARGO = {
    "core": ["cargo", "test", "--quiet", "--locked", "-p", "pw-core",
             "--test", "template_representations", "--", "--format=pretty"],
    "host": ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server",
             "--bin", "pw-dev-server", "--",
             "an_opaque_values_representation", "--format=pretty"],
}
SOURCES = {
    "core": ROOT / "compiler/pw-core/tests/template_representations.rs",
    "host": ROOT / "spikes/own-renderer/server/src/main.rs",
}
NAMES = {
    "core": (
        "a_generic_match_binding_reads_its_opaque_representation",
        "a_records_field_named_value_is_a_field",
        "a_representation_is_read_by_the_opaque_values_path",
        "a_speculated_value_read_through_a_view_reads_its_representation",
    ),
    "host": ("tests::an_opaque_values_representation_is_rendered",),
}
BOUND = 1200


def bounded(cmd, **kw):
    """Native status, or None after a timeout; the shared owner reaps children."""
    try:
        result = producer_process.run(cmd, timeout=BOUND, **kw)
        return result.stdout, result.returncode
    except subprocess.TimeoutExpired as expired:
        return expired.output, None


def cargo_result(suite, code, out):
    result = cargo_assertion_results.validate(code, out, root=ROOT,
        sources=[SOURCES[suite]], expected=NAMES[suite])
    return (True, result.passed, result.failed) if result.valid else (False, 0, 0)


def cargo_tests():
    """All five named results, with failures at actual test assertion sites."""
    passed = failed = 0
    for suite, cmd in CARGO.items():
        out, code = bounded(cmd, cwd=ROOT)
        print(out, flush=True)
        valid, good, bad = cargo_result(suite, code, out)
        if not valid:
            return False, 0, 0
        passed += good
        failed += bad
    return True, passed, failed


def build():
    """Restore the original campaign's store, feed and host build outputs."""
    commands = [
        (["bash", "spikes/own-renderer/run.sh"], {**os.environ, "BUILD_ONLY": "1"}),
        (["bash", "spikes/own-renderer/feed.sh"], dict(os.environ)),
        (["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], dict(os.environ)),
    ]
    for cmd, env in commands:
        out, code = bounded(cmd, cwd=ROOT, env=env)
        if code != 0:
            print(out, flush=True)
            return False
    return True


SUITES = {"cargo": cargo_tests}


def main():
    import mutation_baseline  # keep native output before running any producer
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    watch = mutation_baseline.mutation_bound.start()
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
            print(f"{what}: REFUSED (anchor not unique in {path.name})", flush=True)
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
            return 1
    if not build() or watch.stopped:
        print("REFUSED: restored source did not build", flush=True)
        refused += 1
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived", flush=True)
    return int(bool(refused or survivors))


if __name__ == "__main__":
    sys.exit(main())