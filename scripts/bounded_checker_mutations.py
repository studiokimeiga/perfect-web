#!/usr/bin/env python3
"""Mutation controls for 4c, a read is bounded by construction (track
`bounded-checker`): PW0405, PW0406, PW0407, the `set` pair and the `sets`
clause.

Each mutant undoes one piece:
- the syntax: `set` and `sets` clause heads, and only where a name follows;
- `sets`' values: its words and `chunked`'s size;
- the call graph the effect walk records, a pure call's too;
- the loops: a combinator by its signature, a function of the program's
  that applies its callback (passed on, or called in a loop), a callback
  bound by a `let`, a `for`, a recursion over data, a parameter it passes
  along whole, and the recursive call that is the recursion's own turn;
- the keys: through a field, a `let`, a match arm, a pure call and no
  other read, from a loop's bindings only; a host read's parameters, and a
  helper's keyed parameters;
- a function named as a callback;
- PW0406: a source that states no `sets` is `per_key`, and only `per_key`
  warns;
- the pair's shape: its parameter, its answer, its source, a keyed read
  that reads, and the rule that refuses one that does not fit (PW0407, its
  own, and no policy's value);
- the reads per element kiokun-site is known to make, and only those;
- what a build writes of a source's `sets`, and reads of an older one's;
- the rule's place in `check_units`.

Every mutant must fail `tests/bounded_reads.rs`.

Run from the repository root; `just e14-bounded-reads` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BOUNDED = ROOT / "compiler/pw-core/src/bounded.rs"
EFFECTS = ROOT / "compiler/pw-core/src/effects.rs"
CHECK = ROOT / "compiler/pw-core/src/check.rs"
POLICY = ROOT / "compiler/pw-core/src/policy.rs"
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "a set pair accepts any field of the key's type",
        BOUNDED,
        "                        field.is_none_or(|field| name.as_str() == field)\n",
        "                        field.is_none_or(|_| true)\n",
    ),
    (
        "a set pair drops each key's Result wrapper",
        BOUNDED,
        "|| (holds(e, key, key_field) && (holds(e, found, None) || holds(e, result, None)))",
        "|| (holds(e, key, key_field) && holds(e, found, None))",
    ),
    (
        "`sets` is no clause",
        GRAMMAR,
        "    // begin with.\n    \"sets\",\n",
        "    // begin with.\n",
    ),
    (
        "`set` is no clause",
        GRAMMAR,
        "    \"set\",\n",
        "",
    ),
    ('`set` and `sets` head a clause wherever they are written', GRAMMAR,
     '    "layout", "set", "sets", "row", "order", "index", "unique", "refers", "window", "asset",\n    "migrate",\n',
     '    "layout", "row", "order", "index", "unique", "refers", "window", "asset",\n    "migrate",\n'),
    (
        "`sets` takes any value",
        POLICY,
        "        \"sets\" => Domain::Operator(SETS_OPS, &[\"one_call\", \"per_key\"]),\n",
        "        \"sets\" => Domain::SetRead,\n",
    ),
    (
        "a chunk is of any size",
        POLICY,
        "            [v] if !kind.fits(v) => Some(ValueFault::ArgumentKind(\n",
        "            [v] if false && !kind.fits(v) => Some(ValueFault::ArgumentKind(\n",
    ),
    (
        "the walk records no call",
        EFFECTS,
        "            if let Some(callee) = called {\n",
        "            if let Some(callee) = called.filter(|_| false) {\n",
    ),
    (
        "the walk records no pure call",
        EFFECTS,
        "            if let Some(callee) = called {\n",
        "            if let Some(callee) = called.filter(|_| !effects.is_empty()) {\n",
    ),
    (
        "no combinator applies its callback to its list's elements",
        BOUNDED,
        "            if takes.iter().any(|t| t.same_as(element)) {\n",
        "            if false && takes.iter().any(|t| t.same_as(element)) {\n",
    ),
    (
        "a function of the program's applies no callback",
        BOUNDED,
        "            let found = self.applied_in(cx);\n",
        "            let found: BTreeMap<usize, Option<usize>> =\n"
        "                self.applied_in(cx).into_iter().filter(|_| false).collect();\n",
    ),
    (
        "a callback called in a loop is applied once",
        BOUNDED,
        "            let Some(j) = param(*callee) else {\n",
        "            let Some(j) = param(*callee).filter(|_| false) else {\n",
    ),
    (
        "a callback bound by a `let` is in no loop",
        BOUNDED,
        "                let Some(lambda) = Self::let_lambda(cx, *arg) else {\n",
        "                let Some(lambda) = Self::let_lambda(cx, *arg).filter(|_| false) else {\n",
    ),
    (
        "a `for` binds nothing",
        BOUNDED,
        "                    bindings: pat.map(|p| binds(body, p)).unwrap_or_default(),\n",
        "                    bindings: pat.map(|_| BTreeSet::new()).unwrap_or_default(),\n",
    ),
    (
        "nothing recurses over data",
        BOUNDED,
        "            let recursive = out.recursion();\n",
        "            let recursive: BTreeMap<DefId, (BTreeSet<usize>, ExprId)> = BTreeMap::new();\n",
    ),
    (
        "a parameter passed along whole is walked",
        BOUNDED,
        "                        if whole {\n",
        "                        if false && whole {\n",
    ),
    (
        "a parameter handed whole to a position another declaration walks is not walked",
        BOUNDED,
        "                if lands && !walked {\n",
        "                if false && lands && !walked {\n",
    ),
    (
        "a recursive call is a read per element",
        BOUNDED,
        "                || self.recurses(cx, c)\n",
        "                || (self.recurses(cx, c) && false)\n",
    ),
    (
        "a key through a `let` comes from nothing",
        BOUNDED,
        "                } => self.depends(cx, *init, wanted, elements, seen),\n",
        "                } => {\n                    let _ = init;\n                    None\n                }\n",
    ),
    (
        "a key through a field comes from nothing",
        BOUNDED,
        "            Expr::Field { base, .. } => self.depends(cx, *base, wanted, elements, seen),\n",
        "            Expr::Field { .. } => None,\n",
    ),
    (
        "a key through a match arm comes from nothing",
        BOUNDED,
        "                Expr::Match { scrutinee, .. } => {\n"
        "                    self.depends(cx, *scrutinee, wanted, elements, seen)\n"
        "                }\n",
        "                Expr::Match { scrutinee, .. } => {\n"
        "                    let _ = scrutinee;\n"
        "                    None\n"
        "                }\n",
    ),
    (
        "a key passes through another read",
        BOUNDED,
        "                if cx.calls.get(&e).is_some_and(|c| !c.effects.is_empty()) {\n",
        "                if false && cx.calls.get(&e).is_some_and(|c| !c.effects.is_empty()) {\n",
    ),
    (
        "any read in a loop is refused, whatever its key",
        BOUNDED,
        "                    let wanted = |b: Binder| region.bindings.contains(&b);\n",
        "                    let wanted = |b: Binder| region.bindings.contains(&b) || b == b;\n",
    ),
    (
        "a host read keys nothing",
        BOUNDED,
        "                if decl.kind != DeclKind::Fn || decl.body.is_some() || decl.policy(\"host\").is_none()\n",
        "                if decl.kind != DeclKind::Fn || decl.body.is_some() || decl.policy(\"host\").is_some()\n",
    ),
    (
        "a host call that also writes is no read",
        BOUNDED,
        "                    .find_map(|e| database_domain(&e.written, \"read\"))\n",
        "                    .find_map(|e| database_domain(&e.written, \"read\"))\n"
        "                    .filter(|_| {\n"
        "                        !decl\n"
        "                            .declared_effects\n"
        "                            .iter()\n"
        "                            .flatten()\n"
        "                            .any(|e| database_domain(&e.written, \"write\").is_some())\n"
        "                    })\n",
    ),
    (
        "a helper's parameter keys nothing",
        BOUNDED,
        "                                out.entry(cx.def).or_default().insert(i, reached.clone());\n",
        "                                let _ = &reached;\n",
    ),
    (
        "a function named as a callback is none",
        BOUNDED,
        "        (self.decl_of(def).kind == DeclKind::Fn).then_some(def)\n",
        "        (self.decl_of(def).kind != DeclKind::Fn).then_some(def)\n",
    ),
    (
        "a source that states no `sets` batches",
        BOUNDED,
        "        let Some(value) = written.map(str::trim) else {\n            return Sets::PerKey;\n",
        "        let Some(value) = written.map(str::trim) else {\n            return Sets::OneCall;\n",
    ),
    (
        "a source that batches is warned and one that cannot is not",
        BOUNDED,
        "                if Sets::of(source.sets.as_deref()) != Sets::PerKey {\n",
        "                if Sets::of(source.sets.as_deref()) == Sets::PerKey {\n",
    ),
    (
        "a resource no source holds is the first source's",
        BOUNDED,
        "                    .find(|s| s.holds.iter().any(|(h, _)| h == resource))\n",
        "                    .find(|s| s.holds.iter().any(|(h, _)| h == resource))\n"
        "                    .or_else(|| self.sources.first())\n",
    ),
    (
        "a chunked set read costs one call per chunk, rounded down",
        BOUNDED,
        "            Sets::Chunked(k) => n.div_ceil(k),\n",
        "            Sets::Chunked(k) => n / k,\n",
    ),
    (
        "a set read takes any parameters",
        BOUNDED,
        "        if !listed_key || !rest {\n",
        "        if !listed_key && !rest {\n",
    ),
    (
        "a set read answers anything",
        BOUNDED,
        "        let fits = answers.is_some_and(|e| {\n",
        "        let fits = answers.is_some() || answers.is_some_and(|e| {\n",
    ),
    (
        "a set read reads any source",
        BOUNDED,
        "        if theirs.is_empty() || mine != other {\n",
        "        if theirs.is_empty() {\n",
    ),
    (
        "a keyed read that reads no source has a set read",
        BOUNDED,
        "        if reads.is_empty() {\n",
        "        if reads.is_empty() && reads.len() > 1 {\n",
    ),
    (
        "a set that does not fit is refused as a policy's value",
        BOUNDED,
        "                            code: codes::SET_PAIR_SHAPE.id,\n",
        "                            code: codes::POLICY_VALUE.id,\n",
    ),
    (
        "a set that does not fit states a policy value's invariant",
        BOUNDED,
        "                            invariant: codes::SET_PAIR_SHAPE.invariant,\n",
        "                            invariant: codes::POLICY_VALUE.invariant,\n",
    ),
    (
        "a known read per element is refused",
        BOUNDED,
        "            found.severity = Severity::Warning;\n",
        "            found.severity = Severity::Error;\n",
    ),
    (
        "every read per element is known",
        BOUNDED,
        "            .find(|(a, r, _)| *a == at && *r == read)\n",
        "            .find(|(a, r, _)| !a.is_empty() || (at, read) == (*a, *r))\n",
    ),
    (
        "a build writes no source's cost",
        CHECK,
        "            sets: crate::bounded::Sets::of(s.sets.as_deref()).written(),\n",
        "            sets: crate::bounded::Sets::PerKey.written(),\n",
    ),
    (
        "an older build's clauses are read as no clauses",
        CHECK,
        "    #[serde(default = \"per_key\")]\n",
        "",
    ),
    (
        "the rules run on no unit",
        CHECK,
        "            out.extend(bounded.check(i, &u.src));\n",
        "            let _ = (&bounded, i);\n",
    ),
    ('generated keyed reads have no set pair', BOUNDED, '                        pairs.insert(Self::def(unit, *id), Self::def(unit, *set));\n', '                        let _ = (id, set);\n'),
    ('generated index pairs ignore their queried fields', BOUNDED, '                            && fields == by\n                            && (!primitive.related()', '                            && (fields == by || true)\n                            && (!primitive.related()'),
    ('generated index reads are paired with a prefix batch', BOUNDED, '                        Primitive::Index => Primitive::IndexMany,\n', '                        Primitive::Index => Primitive::PrefixMany,\n'),
    ('generated flat batches replace each individual read in the repair', BOUNDED, '             then give each turn its matching {entries} from the flat result (or {missing} for a \\\n', '             then give every turn the entire {entries} batch from the flat result (or {missing} for a \\\n'),
    ('generated composite keys are described as scalar keys', BOUNDED, '        let input = if held.len() == 1 {\n', '        let input = if true || held.len() == 1 {\n'),
]

TESTS = [
    ['cargo', 'test', '--quiet', '--locked', '-p', 'pw-core', '--test', 'bounded_reads'],
    ['cargo', 'test', '--quiet', '--locked', '-p', 'pw-core', '--test', 'bounded_tables'],
]


def run():
    from cargo_test_results import parse
    passed = failed = 0
    for command in TESTS:
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
        outcome = parse(result.returncode, result.stdout + result.stderr)
        if not outcome.valid:
            return False, 0, 0
        passed += outcome.passed
        failed += outcome.failed
    return True, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    built, passed, failed = run()
    print(f"baseline: {passed} passed, {failed} failed")
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1
    survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = run()
        finally:
            path.write_text(original)
        if not built:
            verdict = "REFUSED (no consistent assertion-failure result)"
            survivors += 1
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
