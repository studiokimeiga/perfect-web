#!/usr/bin/env python3
"""Mutation controls for ADR-0411: the two renderers give one
answer.

Each mutant makes one renderer answer differently, on purpose: an escaping that
misses a character, a scheme that is let through, a key that is not refused, a
change that is not told. A mutant of the browser's and a mutant of the server's
must each fail `spikes/own-renderer/renderer-agreement.test.mjs`, which asks
both the same requests. That is what makes two implementations of a function
that escapes safe to have: a difference is a red run, not a surprise.

Run from the repository root; `just e14-browser-renderer` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"
ESCAPE = ROOT / "runtime/pw-render/src/escape.rs"
SERVER = ROOT / "runtime/pw-render/src/lib.rs"

# How long one command may run; past it, the run counts as failing.
BOUND = 900

# (what, file, anchor, replacement)
MUTANTS = [
    # --- each context's escaping -------------------------------------------
    ("text does not escape <", RUNTIME, "const escText = (v) => v.replace(/[&<>]/g, entity);", "const escText = (v) => v.replace(/[&>]/g, entity);"),
    ("text does not escape &", RUNTIME, "const escText = (v) => v.replace(/[&<>]/g, entity);", "const escText = (v) => v.replace(/[<>]/g, entity);"),
    ("an attribute does not escape a double quote", RUNTIME, """const escAttribute = (v) => v.replace(/[&"'<>`]/g, entity);""", """const escAttribute = (v) => v.replace(/[&'<>`]/g, entity);"""),
    ("an attribute does not escape a single quote", RUNTIME, """const escAttribute = (v) => v.replace(/[&"'<>`]/g, entity);""", """const escAttribute = (v) => v.replace(/[&"<>`]/g, entity);"""),
    ("an attribute does not escape a backtick", RUNTIME, """const escAttribute = (v) => v.replace(/[&"'<>`]/g, entity);""", """const escAttribute = (v) => v.replace(/[&"'<>]/g, entity);"""),
    ("a form control's first newline is not doubled", RUNTIME, """const escContent = (v) => (/^[\\n\\r]/.test(v) ? "\\n" : "") + escText(v);""", """const escContent = (v) => escText(v);"""),
    ("a form control's first carriage return is not doubled", RUNTIME, """const escContent = (v) => (/^[\\n\\r]/.test(v) ? "\\n" : "") + escText(v);""", """const escContent = (v) => (/^[\\n]/.test(v) ? "\\n" : "") + escText(v);"""),
    ("what an address drops is what JavaScript calls space", RUNTIME, "const UNSEEN = /[\\p{White_Space}\\p{Cc}]/gu;", "const UNSEEN = /[\\s\\p{Cc}]/gu;"),
    ("an address keeps its control characters", RUNTIME, "const UNSEEN = /[\\p{White_Space}\\p{Cc}]/gu;", "const UNSEEN = /[\\p{White_Space}]/gu;"),
    ("a data: address is let through", RUNTIME, """const DANGEROUS = ["javascript:", "vbscript:", "data:"];""", """const DANGEROUS = ["javascript:", "vbscript:"];"""),
    ("a vbscript: address is let through", RUNTIME, """const DANGEROUS = ["javascript:", "vbscript:", "data:"];""", """const DANGEROUS = ["javascript:", "data:"];"""),
    ("an address's references are not read", RUNTIME, "const decoded = decodeEntities(lowerAscii(v.replace(UNSEEN, \"\")));", "const decoded = lowerAscii(v.replace(UNSEEN, \"\"));"),
    ("an address's case is kept", RUNTIME, "const decoded = decodeEntities(lowerAscii(v.replace(UNSEEN, \"\")));", "const decoded = decodeEntities(v.replace(UNSEEN, \"\"));"),
    ("a reference's window is a byte longer", RUNTIME, """      const end = rest.slice(0, 12).indexOf(";");\n""", """      const end = rest.slice(0, 13).indexOf(";");\n"""),
    ("a reference past a code point is read", RUNTIME, "    if (code === null || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) return null;\n", "    if (code === null) return null;\n    if (code > 0x10ffff) return \"\";\n"),
    ("a reference to what a parser drops is kept", RUNTIME, "    return c.replace(UNSEEN, \"\");\n", "    return c;\n"),
    ("a reference's plus sign is refused", RUNTIME, """if (body.startsWith("#x")) code = digits(body.slice(2), 16, /^\\+?[0-9a-f]+$/);""", """if (body.startsWith("#x")) code = digits(body.slice(2), 16, /^[0-9a-f]+$/);"""),
    ("a colon's name is not read", RUNTIME, """else return body === "colon" ? ":" : body === "amp" ? "&" : null;""", """else return body === "amp" ? "&" : null;"""),
    ("a component keeps a character it should encode", RUNTIME, "b === 45 || b === 46 || b === 95 || b === 126;", "b === 45 || b === 46 || b === 95 || b === 126 || b === 33;"),
    ("a component's bytes are in lower case", RUNTIME, """`%${b.toString(16).toUpperCase().padStart(2, "0")}`""", """`%${b.toString(16).padStart(2, "0")}`"""),
    ("a style's expression( is kept", RUNTIME, """      lower.includes("expression(") || DANGEROUS.some""", """      DANGEROUS.some"""),
    ("a style's quoted url( is kept", RUNTIME, """lower.includes(`url(${s}`) || lower.includes(`url('${s}`));""", """lower.includes(`url(${s}`));"""),
    # --- what a value is ------------------------------------------------------
    ("true is written otherwise", RUNTIME, """        return v ? "true" : "false";\n""", """        return v ? "True" : "false";\n"""),
    ("a list has a text form", RUNTIME, """        return v === null || v === undefined ? "" : null;\n""", """        return v === null || v === undefined ? "" : String(v);\n"""),
    ("zero holds", RUNTIME, """        return Number.isFinite(v) && v !== 0;\n""", """        return Number.isFinite(v);\n"""),
    ("an empty list holds", RUNTIME, """        return isList(v) ? v.length > 0 : v !== null && v !== undefined;\n""", """        return isList(v) ? true : v !== null && v !== undefined;\n"""),
    ("a case is a condition", RUNTIME, """    if (isVariant(v)) throw unrepresented("an Option or a Result is taken apart with {#match}");\n""", ""),
    ("a field set whole is not read", RUNTIME, """        const all = field(value, whole);\n        if (all !== NONE) return all;\n""", ""),
    ("a field of a case is read", RUNTIME, "        if (!isRecord(value)) return NONE;\n        const whole =", "        if (!isObject(value)) return NONE;\n        const whole ="),
    ("a key reads through what is no record", RUNTIME, """      if (name === "") continue;\n      if (!isRecord(v)) return "";\n""", """      if (name === "") continue;\n      if (!isObject(v)) return "";\n"""),
    # --- graphs ---------------------------------------------------------------
    ("a node may be held twice", RUNTIME, "      const node = nodes[i];\n      nodes[i] = NONE;\n      return node;\n", "      return nodes[i];\n"),
    ("a node no other holds is kept", RUNTIME, """    if (nodes.slice(1).some((n) => n !== NONE)) throw malformed("a node no other holds");\n""", ""),
    ("a $node outside a graph is a record", RUNTIME, """    if ("$node" in v && Object.keys(v).length === 1) throw malformed("a $node outside any $graph");\n""", ""),
    # --- writing --------------------------------------------------------------
    ("a text part writes no anchors", RUNTIME, "        } else html.push(start, escaped(needText(e, p.value), p.context), end);\n", "        } else html.push(escaped(needText(e, p.value), p.context));\n"),
    ("each handler writes its own attribute", RUNTIME, """          if (q?.chunk !== "dynamic" || q.value.part !== "event" || q.value.owner !== c.value.owner) break;\n""", "          break;\n"),
    ("an element's handlers run together with another's", RUNTIME, """q.value.part !== "event" || q.value.owner !== c.value.owner) break;""", """q.value.part !== "event") break;"""),
    ("an absent boolean attribute leaves its space", RUNTIME, "        else dropSpace(html);\n", "        else;\n"),
    ("a false boolean attribute is written", RUNTIME, """        return [p.name, holds(need(e, p.value)) ? "" : null];\n""", """        return [p.name, ""];\n"""),
    ("a streamed region with no answer is refused", RUNTIME, "        if (!p.streamed) throw missing(`the answer of stream ${p.id}`);\n", "        throw missing(`the answer of stream ${p.id}`);\n"),
    ("a region with no answer shows its placeholder", RUNTIME, "        if (!p.streamed) throw missing(`the answer of stream ${p.id}`);\n", ""),
    ("raw HTML is written", RUNTIME, """        throw new Blocked("UnauthorisedRawHtml", p.capability);\n""", """        html.push(needText(e, p.value));\n        return;\n"""),
    ("a blocked part writes nothing", RUNTIME, """      case "blocked":\n        throw unrepresented(p.reason);\n""", """      case "blocked":\n        return;\n"""),
    ("a loop's missing key is rendered", RUNTIME, """    const key = keyOf(item, p.key);\n    if (key === "") throw new Blocked("MissingLoopKey", p.key);\n""", """    const key = keyOf(item, p.key);\n"""),
    ("a loop's duplicate key is rendered", RUNTIME, """    if (seen.has(key)) throw new Blocked("DuplicateLoopKey", key);\n    seen.add(key);\n    return key;\n""", """    seen.add(key);\n    return key;\n"""),
    ("a row is kept whatever a block around it bound", RUNTIME, """      if (!a || !b || a.name !== b.name || !same(a.value, b.value)) return false;\n""", """      if (!a || !b || a.name !== b.name) return false;\n"""),
    # Not "a row is kept whichever names a block around it bound": the names
    # around a list are its template's, the same at every render of it, so a
    # mutant that drops their comparison changes nothing a page can show (run
    # 42772395298 found it surviving). The comparison of what they hold is the
    # one above.
    ("a loop that is no list renders nothing", RUNTIME, "    if (!isList(items)) throw missing(p.collection);\n", "    if (!isList(items)) return;\n"),
    ("a nested instance may pass what a browser nests", RUNTIME, "  const NESTED_ELEMENTS = 500;\n", "  const NESTED_ELEMENTS = 501;\n"),
    ("an instance counts no elements around it", RUNTIME, "    const root = e.elements + (p.elements ?? 0);\n", "    const root = e.elements;\n"),
    ("an instance reads the page's values", RUNTIME, """    let child = { page: new Map(), names: null, frames: [...e.frames, { scope: p.id, instance: token }], elements: root };""", """    let child = { page: e.page, names: null, frames: [...e.frames, { scope: p.id, instance: token }], elements: root };"""),
    ("a case with no payload binds nothing", RUNTIME, """      if (!held) throw unrepresented(`${v.$case} has no payload to bind`);\n""", ""),
    ("a case's fields need not be as many", RUNTIME, """      if (parts.length !== fields.length) throw unrepresented(`${v.$case} has another number of fields`);\n""", ""),
    # --- what a handler captures ---------------------------------------------
    ("an integer past 2^53 is captured", RUNTIME, "? v > 2n ** 53n || v < -(2n ** 53n) : Math.abs(v) > 2 ** 53) {", "? false : Math.abs(v) > 2 ** 53) {"),
    ("what a host set on a row is captured", RUNTIME, """      return new Map(Object.keys(v).filter((k) => !k.includes(".")).map((k) => [k, captured(v[k])]));\n""", """      return new Map(Object.keys(v).map((k) => [k, captured(v[k])]));\n"""),
    ("a case is captured", RUNTIME, """    if (isVariant(v)) throw unrepresented(`${v.$case} cannot be captured by a handler`);\n""", ""),
    ("keys sort as JavaScript's strings do", RUNTIME, """    return `{${[...v.keys()].sort(byPoint).map(""", """    return `{${[...v.keys()].sort().map("""),
    ("captures are not escaped", RUNTIME, "  const escapedCaptures = (text) => (text === null ? null : escAttribute(text));\n", "  const escapedCaptures = (text) => text;\n"),
    ("one name from two places is not refused", RUNTIME, """        if (paths.has(path) && paths.get(path) !== at) {\n          throw unrepresented("two handlers on one element capture one name from two places");\n        }\n""", ""),
    ("a rename is not followed", RUNTIME, """        if (Object.hasOwn(renames, root)) at = dot < 0 ? renames[root] : `${renames[root]}${path.slice(dot)}`;\n""", ""),
    ("one path through another's value is taken", RUNTIME, """    if (at.has(leaf)) throw unrepresented("one capture path runs through another's value");\n""", ""),
    # --- what changed ----------------------------------------------------------
    ("a changed text is not told", RUNTIME, "            out.push([p.id, { text: y }]);\n", ""),
    ("a changed attribute is not told", RUNTIME, "          if (x[0] !== y[0] || x[1] !== y[1]) out.push([p.id, { attribute: y[0], value: y[1] }]);\n", ""),
    ("what a handler captures is not told", RUNTIME, """          if (x !== y) out.push([p.id, { attribute: "data-pw-captures", value: y }]);\n""", ""),
    ("a block that shows another branch is set in place", RUNTIME, "          if (x !== y || !changesWithin(x ? p.then : p.otherwise, before, after, handlers, others, out)) return false;\n", "          if (!changesWithin(x ? p.then : p.otherwise, before, after, handlers, others, out)) return false;\n"),
    ("a match that shows another arm is set in place", RUNTIME, "          if (x.case !== y.case || !changesWithin(y.body, was, now, handlers, others, out)) return false;\n", "          if (!changesWithin(y.body, was, now, handlers, others, out)) return false;\n"),
    ("a changed list in a row is not told", RUNTIME, "            if (!same(x, y) && listChanged(p, x, y, after, others)) out.push([p.id, { list: true }]);\n", ""),
    ("a list's change is told as the row's, set in place", RUNTIME, "      return found && found.some(([, change]) => change.list) ? null : found;\n", "      return found;\n"),
    ("two values of another type are one", RUNTIME, """  const scalar = (v) => (typeof v === "boolean" ? "b" : isInt(v) ? "i" : "t") + shown(v);\n""", """  const scalar = (v) => shown(v);\n"""),
    # --- a list inside a row ---------------------------------------------------
    ("a new list's missing key is taken", RUNTIME, """        if (key === "") throw new Blocked("MissingLoopKey", p.key);\n        if (seen.has(key)) throw new Blocked("DuplicateLoopKey", key);\n        seen.add(key);\n      }\n""", """        if (seen.has(key)) throw new Blocked("DuplicateLoopKey", key);\n        seen.add(key);\n      }\n"""),
    ("a new list's repeated key is taken", RUNTIME, """        if (seen.has(key)) throw new Blocked("DuplicateLoopKey", key);\n        seen.add(key);\n      }\n""", """        seen.add(key);\n      }\n"""),
    ("an item gone from a list is no change", RUNTIME, """        if (wanted.includes(t)) current.push([t, v]);\n        else changed = true;\n""", """        if (wanted.includes(t)) current.push([t, v]);\n"""),
    ("an item new to a list is no change", RUNTIME, """          current.splice(i, 0, [wanted[i], item]);\n          changed = true;\n""", """          current.splice(i, 0, [wanted[i], item]);\n"""),
    ("an item new to a list is not rendered", RUNTIME, """          eachInstance(p, item, e, others);\n          current.splice(i, 0, [wanted[i], item]);\n""", """          current.splice(i, 0, [wanted[i], item]);\n"""),
    ("an item moved in a list is no change", RUNTIME, """          current.splice(i, 0, current.splice(found, 1)[0]);\n          changed = true;\n""", """          current.splice(i, 0, current.splice(found, 1)[0]);\n"""),
    ("an item of a list that changed is no change", RUNTIME, """          current[i][1] = item;\n          changed = true;\n""", """          current[i][1] = item;\n"""),
    ("an item of a list rendered again is not rendered", RUNTIME, """        if (rowChanges(p, opened(current[i][1]), item, e, others) === null) eachInstance(p, item, e, others);\n""", """        rowChanges(p, opened(current[i][1]), item, e, others);\n"""),
    # --- and the server's own, so a drift there is as loud ---------------------
    ("the server's text does not escape >", ESCAPE, """            '<' => out.push_str("&lt;"),\n            '>' => out.push_str("&gt;"),\n            _ => out.push(c),\n        }\n    }\n    out\n}\n\n/// **A form control's value""", """            '<' => out.push_str("&lt;"),\n            _ => out.push(c),\n        }\n    }\n    out\n}\n\n/// **A form control's value"""),
    ("the server's attribute does not escape a backtick", ESCAPE, """            '`' => out.push_str("&#96;"),\n""", ""),
    ("the server lets a data: address through", ESCAPE, """const DANGEROUS_SCHEMES: &[&str] = &["javascript:", "vbscript:", "data:"];""", """const DANGEROUS_SCHEMES: &[&str] = &["javascript:", "vbscript:"];"""),
    ("the server's false boolean attribute leaves its space", SERVER, """            } else if out.ends_with(' ') {\n""", """            } else if false {\n"""),
    ("the server takes a new list's repeated key", SERVER, """        if !seen.insert(raw.clone()) {\n""", """        if !seen.insert(raw.clone()) && false {\n"""),
    ("the server's duplicate key is rendered", SERVER, """                        if seen.insert(raw.clone(), raw.clone()).is_some() {\n""", """                        if false && seen.insert(raw.clone(), raw.clone()).is_some() {\n"""),
]


def bounded(cmd, **kw):
    """(output, returncode), or (output, None) when it ran past the bound."""
    p = subprocess.Popen(cmd, start_new_session=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, **kw)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def agreement():
    """(built, passed, failed): the oracle built from the source, and the suite."""
    out, code = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-render-oracle", "--bin", "oracle"], cwd=ROOT)
    if code != 0:
        return False, 0, 0
    out, code = bounded(["node", "--test", "--test-reporter=spec", "spikes/own-renderer/renderer-agreement.test.mjs"], cwd=ROOT)
    if code is None:
        return True, 0, 1
    passed = sum(int(n) for n in re.findall(r"(?m)^ℹ pass (\d+)$", out))
    failed = sum(int(n) for n in re.findall(r"(?m)^ℹ fail (\d+)$", out))
    return passed + failed > 0, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built, passed, failed = agreement()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
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
            built, passed, failed = agreement()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
    # The oracle is built again from the restored source.
    agreement()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
