#!/usr/bin/env python3
"""Mutation controls for the device runtime's ADR: a page that talks to no
server starts a runtime with no server's code in it.

Each mutant undoes one piece:
- the decision: each thing it asks of a manifest and of the table, each answer,
  each recovery, and the order it asks in;
- the runtime: a guard that keeps a server's code out, the page's use of the
  decision, the source's flag;
- the packaging: the flag's replacing, the tree shaking, what it writes;
- the host: which page runs on the device, what its document asks for, what
  its policy allows, what a build must say and hold;
- the suite: what makes a production build one of this runtime.

A mutant of the decision must fail `decision-agreement.test.mjs`, of the packaging
or a guard `optimize-assets.test.mjs` (or `host-plan.test.mjs`), of the host
`tests::page_bundle::` or the production document's test, and of the page's use
`e2e/device-runtime.spec.mjs` in Chromium against a production build, and of
what a block keeps `e2e/nested-blocks.spec.mjs` against a production build of
the demos.

Run from the repository root; `just e14-device-runtime` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGING = ROOT / "spikes/own-renderer/optimize-assets.mjs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
CSP = ROOT / "spikes/own-renderer/server/src/csp.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"
HOSTS = ROOT / "spikes/own-renderer/host-metadata.mjs"

# How long one command may run; past it, the run counts as failing.
BOUND = 900

CODE = """    const code = !captures && capture
      ? 9
      : !/^\\+?0*2$/.test(scheme)
        ? 2
        : !/^\\+?0*1$/.test(abi)
          ? 3
          : !known.has(`${handler}|${capture}`)
            ? 4
            : told && scope !== "public" && scope !== told[3]
              ? 5
              : told && schema !== told[2]
                ? 6
                : 0;
"""


def reordered(*order):
    """The decision's questions, asked in `order` (each a code)."""
    question = {
        9: "!captures && capture",
        2: "!/^\\+?0*2$/.test(scheme)",
        3: "!/^\\+?0*1$/.test(abi)",
        4: "!known.has(`${handler}|${capture}`)",
        5: 'told && scope !== "public" && scope !== told[3]',
        6: "told && schema !== told[2]",
    }
    return "    const code = " + " : ".join(f"{question[c]} ? {c}" for c in order) + " : 0;\n"


# (what, suite, file, anchor, replacement)
MUTANTS = [
    # -- the decision
    ("a `#` line of the table is a handler", "decision", RUNTIME,
     'const known = new Set(lines.filter((l) => l && l[0] !== "#")', "const known = new Set(lines.filter((l) => l)"),
    ("an empty line of the table is a handler", "decision", RUNTIME,
     'const known = new Set(lines.filter((l) => l && l[0] !== "#")', 'const known = new Set(lines.filter((l) => l[0] !== "#")'),
    ("a handler that captures nothing is not known", "decision", RUNTIME,
     '.map((l) => (l.includes("|") ? l : `${l}|`)));', ".map((l) => l));"),
    ("a line that begins as this page's is this page's", "decision", RUNTIME,
     "const told = lines.find((l) => l.startsWith(`#page|${page}|`))?.split(\"|\");",
     "const told = lines.find((l) => l.startsWith(`#page|${page}`))?.split(\"|\");"),
    ("no bytes under a schema is not malformed", "decision", RUNTIME,
     "    const code = !captures && capture\n      ? 9\n", "    const code = false\n      ? 9\n"),
    ("no bytes is malformed under any schema", "decision", RUNTIME,
     "    const code = !captures && capture\n      ? 9\n", "    const code = !captures\n      ? 9\n"),
    ("any scheme is understood", "decision", RUNTIME,
     "      : !/^\\+?0*2$/.test(scheme)\n", "      : false\n"),
    ("only a bare 2 is the scheme", "decision", RUNTIME,
     "      : !/^\\+?0*2$/.test(scheme)\n", '      : scheme !== "2"\n'),
    ("a scheme that begins with 2 is understood", "decision", RUNTIME,
     "      : !/^\\+?0*2$/.test(scheme)\n", "      : !/^\\+?0*2/.test(scheme)\n"),
    ("a scheme with a sign is not understood", "decision", RUNTIME,
     "      : !/^\\+?0*2$/.test(scheme)\n", "      : !/^0*2$/.test(scheme)\n"),
    ("any ABI is supported", "decision", RUNTIME,
     "        : !/^\\+?0*1$/.test(abi)\n", "        : false\n"),
    ("only a bare 1 is the ABI", "decision", RUNTIME,
     "        : !/^\\+?0*1$/.test(abi)\n", '        : abi !== "1"\n'),
    ("an ABI with a sign is not supported", "decision", RUNTIME,
     "        : !/^\\+?0*1$/.test(abi)\n", "        : !/^0*1$/.test(abi)\n"),
    ("an unknown handler attaches", "decision", RUNTIME,
     "          : !known.has(`${handler}|${capture}`)\n", "          : false\n"),
    ("a handler is known by its name, whatever it captures", "decision", RUNTIME,
     "          : !known.has(`${handler}|${capture}`)\n",
     "          : ![...known].some((k) => k.startsWith(`${handler}|`))\n"),
    ("a wider scope attaches", "decision", RUNTIME,
     '            : told && scope !== "public" && scope !== told[3]\n', "            : false\n"),
    ("a public manifest is held to the page's scope", "decision", RUNTIME,
     '            : told && scope !== "public" && scope !== told[3]\n', "            : told && scope !== told[3]\n"),
    ("the page's own scope is refused", "decision", RUNTIME,
     '            : told && scope !== "public" && scope !== told[3]\n', '            : told && scope !== "public"\n'),
    ("told nothing, a scope is still compared", "decision", RUNTIME,
     '            : told && scope !== "public" && scope !== told[3]\n',
     '            : scope !== "public" && scope !== told?.[3]\n'),
    ("another document attaches", "decision", RUNTIME,
     "              : told && schema !== told[2]\n", "              : false\n"),
    ("told nothing, a document is still compared", "decision", RUNTIME,
     "              : told && schema !== told[2]\n", "              : schema !== told?.[2]\n"),
    ("the scheme is asked before the bytes", "decision", RUNTIME, CODE, reordered(2, 9, 3, 4, 5, 6)),
    ("the ABI is asked before the scheme", "decision", RUNTIME, CODE, reordered(9, 3, 2, 4, 5, 6)),
    ("the handler is asked before the ABI", "decision", RUNTIME, CODE, reordered(9, 2, 4, 3, 5, 6)),
    ("the scope is asked before the handler", "decision", RUNTIME, CODE, reordered(9, 2, 3, 5, 4, 6)),
    ("the document is asked before the scope", "decision", RUNTIME, CODE, reordered(9, 2, 3, 4, 6, 5)),
    ("every answer attaches", "decision", RUNTIME,
     "    return { attach: code === 0, recovery, code };", "    return { attach: true, recovery, code };"),
    ("a private slot is refetched", "decision", RUNTIME,
     '    const recovery = construct === "private"\n      ? "rerender-private-slot"\n',
     '    const recovery = construct === "private"\n      ? "refetch-region"\n'),
    ("a pending command is refetched", "decision", RUNTIME,
     '      : construct === "command"\n        ? "retry-interaction"\n',
     '      : construct === "command"\n        ? "refetch-region"\n'),
    ("an open resource is refetched", "decision", RUNTIME,
     '        : construct === "resource"\n          ? "reject-irrecoverable"\n',
     '        : construct === "resource"\n          ? "refetch-region"\n'),
    ("a region of private state is refetched", "decision", RUNTIME,
     '          : scope === "public"\n            ? "refetch-region"\n            : "rerender-private-slot";\n',
     '          : "refetch-region";\n'),
    # -- the page's use of it
    ("a device page decides nothing", "browser", RUNTIME,
     "  decide = decider(lines, PAGE);\n",
     '  decide = () => ({ attach: true, recovery: "reload", code: 0 });\n'),
    ("a device page's table is not heard", "browser", RUNTIME,
     '  const lines = heardTable(await fetch(delivered("/pw-handlers")).then((r) => (r.ok ? r.text() : "")));\n',
     '  const lines = (await fetch(delivered("/pw-handlers")).then((r) => (r.ok ? r.text() : ""))).split("\\n");\n'),
    ("a device page's decision is another page's", "browser", RUNTIME,
     "  decide = decider(lines, PAGE);\n",
     '  decide = decider(lines, "another");\n'),
    # -- which rows changed
    ("every render morphs the whole block", "browser", RUNTIME,
     "    if (before && rowsMade(r, before, html, inserted)) return inserted;\n", ""),
    ("what a block rendered outlives its anchor", "nested", RUNTIME,
     "    const before = lastRendered.get(r.start);\n    lastRendered.set(r.start, html);\n",
     "    const before = (globalThis.kept ??= new Map()).get(id);\n    globalThis.kept.set(id, html);\n"),
    ("a block taken away this pass is rendered", "nested", RUNTIME,
     "  if (!r?.start?.isConnected) return [];\n", "  if (!r?.start) return [];\n"),
    ("a box in a row left alone is left", "browser", RUNTIME,
     '    if (after && before && box.checked !== box.hasAttribute("checked")) box.checked = !box.checked;\n', ""),
    ("a box the page ticked is not looked for", "browser", RUNTIME,
     'querySelectorAll("input:checked:not(:default),input:not(:checked):default")', 'querySelectorAll("input:not(:checked):default")'),
    ("a box the page unticked is not looked for", "browser", RUNTIME,
     'querySelectorAll("input:checked:not(:default),input:not(:checked):default")', 'querySelectorAll("input:checked:not(:default)")'),
    ("a box before the block is written", "browser", RUNTIME,
     '    if (after && before && box.checked !== box.hasAttribute("checked")) box.checked = !box.checked;\n',
     '    if (before && box.checked !== box.hasAttribute("checked")) box.checked = !box.checked;\n'),
    ("a box after the block is written", "browser", RUNTIME,
     '    if (after && before && box.checked !== box.hasAttribute("checked")) box.checked = !box.checked;\n',
     '    if (after && box.checked !== box.hasAttribute("checked")) box.checked = !box.checked;\n'),
    ("a new row is taken for the first", "contract", RUNTIME,
     "  return b.map((row) => at.get(row.key) ?? -1);\n", "  return b.map((row) => at.get(row.key) ?? 0);\n"),
    ("a new row counts among those that stay", "contract", RUNTIME,
     "    if (value < 0) return;\n", ""),
    ("only the shortest run stays", "contract", RUNTIME,
     "  for (let i = tails.length > 0 ? tails[tails.length - 1] : -1; i >= 0; i = before[i]) out.add(i);\n",
     "  for (let i = tails.length > 0 ? tails[0] : -1; i >= 0; i = before[i]) out.add(i);\n"),
    ("a row out of place is left there", "rows", RUNTIME,
     "      if (!still.has(i)) moveNodes(parent, between(nodes[0], nodes[1]), anchor);\n", ""),
    ("a row that went stays", "rows", RUNTIME,
     "      rowGone(shown.get(row.key));\n", ""),
    ("when no row stays they all do", "rows", RUNTIME,
     "    rowsAllGone(range);\n", ""),
    ("a row changed in place is left", "rows", RUNTIME,
     "      if (a[i] !== b[i] && a[i].text !== b[i].text) changed.push([shown.get(b[i].key), b[i]]);\n", ""),
    ("a row changed and moved is left", "rows", RUNTIME,
     "    if (old && old !== row && old.text !== row.text) changed.push([shown.get(row.key), row]);\n", ""),
    ("new rows go in at the end", "rows", RUNTIME,
     "      parent.insertBefore(content, anchor);\n", "      parent.insertBefore(content, range.end);\n"),
    ("a new row among others goes in at the end", "rows", RUNTIME,
     "      rowPut(parent, nodes, anchor);\n", "      rowPut(parent, nodes, range.end);\n"),
    ("a kept row's text is written again", "rows", RUNTIME,
     "      const row = old && holdsStill(old, item, e) ? old : rowOf(p, e, others, item, key);\n",
     "      const row = rowOf(p, e, others, item, key);\n"),
    ("a row whose value changed is kept", "rows", RUNTIME,
     "    if (!same(row.item, item)) return false;\n", ""),
    ("a row that read the page is kept", "rows", RUNTIME,
     "    for (const [name, value] of row.reads) if (e.page.get(name) !== value) return false;\n", ""),
    ("what a row reads is not noted", "rows", RUNTIME,
     "    if (!PW_SERVER && e.reads) e.reads.set(key, e.page.get(key));\n", ""),
    ("a press on a row's element is not heard", "rows", RUNTIME,
     "    if (owner === undefined) continue;\n    let stop = false;\n", "    if (owner === undefined || el.closest(\"tr\")) continue;\n    let stop = false;\n"),
    ("a handler that stops does not", "rows", RUNTIME,
     '      stop ||= modifiers.includes("stop");\n', ""),
    ("an event that stays on its target rises", "rows", RUNTIME,
     "  for (const el of STAYS_ON_TARGET.has(e.type) ? [e.target] : e.composedPath()) {\n",
     "  for (const el of e.composedPath()) {\n"),
    ("what a row holds is indexed", "rows", RUNTIME,
     "        if (DELEGATED && !view) {\n", "        if (false && DELEGATED && !view) {\n"),
    # -- what keeps a server's code out
    ("the source's flag is off", "packaging", RUNTIME, "const PW_SERVER = true;", "const PW_SERVER = false;"),
    ("a read is asked for", "packaging", RUNTIME,
     "  if (PW_SERVER) readAgain(changed);\n", "  readAgain(changed);\n"),
    ("the subscription starts", "packaging", RUNTIME,
     "  if (PW_SERVER && parts.listens !== false) subscribe();\n", "  if (parts.listens !== false) subscribe();\n"),
    ("a stream is watched", "packaging", RUNTIME,
     "  if (PW_SERVER) watchStreams();\n", "  watchStreams();\n"),
    ("a command can be sent", "packaging", RUNTIME,
     "      command: !PW_SERVER ? null : async (component, args, how = {}) => {\n",
     "      command: async (component, args, how = {}) => {\n"),
    ("a press speculates", "packaging", RUNTIME,
     "  if (PW_SERVER && parts.speculation && !speculationModule && !speculationLoaded) {\n",
     "  if (parts.speculation && !speculationModule && !speculationLoaded) {\n"),
    ("a handler can navigate", "packaging", RUNTIME,
     "      navigate: !PW_SERVER ? null : (route, args) => navigate(route, args, done),\n",
     "      navigate: (route, args) => navigate(route, args, done),\n"),
    ("a link is followed in place", "packaging", RUNTIME,
     "if (PW_SERVER) document.addEventListener(\n  \"click\",\n  (e) => {\n    const a = parts.layout ? linkOf(e) : null;\n",
     "document.addEventListener(\n  \"click\",\n  (e) => {\n    const a = parts.layout ? linkOf(e) : null;\n"),
    ("back and forward are heard", "packaging", RUNTIME,
     "if (PW_SERVER) addEventListener(\n  \"popstate\",\n", "addEventListener(\n  \"popstate\",\n"),
    ("the scroll is taken from the browser", "packaging", RUNTIME,
     'if (PW_SERVER && parts.layout && "scrollRestoration" in history)', 'if (parts.layout && "scrollRestoration" in history)'),
    ("the frame hook holds the frames", "packaging", RUNTIME,
     "if (PW_SERVER) window.__pwTestApply = applyFrame;\n", "window.__pwTestApply = applyFrame;\n"),
    ("the batch hook holds the batches", "packaging", RUNTIME,
     "if (PW_SERVER) window.__pwTestBatch = applyBatch;\n", "window.__pwTestBatch = applyBatch;\n"),
    # -- what a build's device pages use
    ("a value in a URL is not looked for", "packaging", PACKAGING,
     '    if (part.context === "url" || part.context === "style") out.PW_LINKS = true;\n', ""),
    ("a match is not looked for", "packaging", PACKAGING,
     '    if (part.part === "match") out.PW_MATCH = true;\n', ""),
    ("a view is not looked for", "packaging", PACKAGING,
     "      out.PW_VIEWS = true;\n", ""),
    ("a dialog is not looked for", "packaging", PACKAGING,
     '      if (/<dialog[\\s>]/i.test(chunk.value)) out.PW_DIALOGS = true;\n', ""),
    ("a view's own template is not read", "packaging", PACKAGING,
     "        if (view) featuresIn(view.chunks, templates, out, seen);\n", ""),
    ("what is inside a part is not read", "packaging", PACKAGING,
     "      if (inner) featuresIn(inner, templates, out, seen);\n", ""),
    ("a match's arms are not read", "packaging", PACKAGING,
     ", ...(part.arms ?? []).map((a) => a.body)]) {\n      if (inner) featuresIn", "]) {\n      if (inner) featuresIn"),
    ("a page a reader decides is read", "packaging", PACKAGING,
     "    if (plan.prerender !== true || (plan.layout !== undefined && plan.layout !== null)) continue;\n",
     "    if (plan.layout !== undefined && plan.layout !== null) continue;\n"),
    ("a page with a layout is read", "packaging", PACKAGING,
     "    if (plan.prerender !== true || (plan.layout !== undefined && plan.layout !== null)) continue;\n",
     "    if (plan.prerender !== true) continue;\n"),
    ("a build that cannot be read keeps nothing", "packaging", PACKAGING,
     "  if (text === null) return everything;\n",
     '  if (text === null) return { PW_LINKS: false, PW_MATCH: false, PW_VIEWS: false, PW_DIALOGS: false };\n'),
    ("every feature is always kept", "packaging", PACKAGING,
     "  const uses = await deviceFeatures(directory);\n",
     "  const uses = { PW_LINKS: true, PW_MATCH: true, PW_VIEWS: true, PW_DIALOGS: true };\n"),
    # -- the packaging
    ("the flag is not replaced", "packaging", PACKAGING,
     '.replaceAll("PW_SERVER", "false")', '.replaceAll("PW_SERVER", "true")'),
    ("what is unused stays", "packaging", PACKAGING,
     "    minify: true,\n    treeShaking: true,\n", "    minify: true,\n"),
    ("the device's runtime is not compressed", "packaging", PACKAGING,
     '  await writeFile(file + ".br", brotliCompressSync(Buffer.from(code), BR));\n', ""),
    ("the device's runtime is not gzipped", "packaging", PACKAGING,
     '  await writeFile(file + ".gz", gzipSync(Buffer.from(code), { level: 9, mtime: 0 }));\n', ""),
    ("a build does not say it has one", "packaging", PACKAGING,
     "bundled: true, device: true, inline: true }", "bundled: true, device: false, inline: true }"),
    ("a build says the runtime it packed, not what from", "packaging", PACKAGING,
     '  const runtime = await readFile(path.join(directory, "pw-runtime.mjs")).catch(() => null);\n'
     "  for (const asset of assets) await packAsset(directory, asset);\n",
     "  for (const asset of assets) await packAsset(directory, asset);\n"
     '  const runtime = await readFile(path.join(directory, "pw-runtime.mjs")).catch(() => null);\n'),
    ("a build with no runtime fails", "packaging", PACKAGING,
     '  const runtime = await readFile(path.join(directory, "pw-runtime.mjs")).catch(() => null);\n',
     '  const runtime = await readFile(path.join(directory, "pw-runtime.mjs"));\n'),
    # -- the suite
    ("a production build is of any runtime", "packaging", HOSTS,
     '    return packed === createHash("sha256").update(runtime).digest("hex");\n', "    return true;\n"),
    ("a production build is of none", "packaging", HOSTS,
     '    return packed === createHash("sha256").update(runtime).digest("hex");\n', "    return false;\n"),
    ("a delivery that cannot be read is of any runtime", "packaging", HOSTS,
     "  } catch {\n    return false;\n  }\n}\n\nexport function buildProblem",
     "  } catch {\n    return true;\n  }\n}\n\nexport function buildProblem"),
    # -- the host
    ("every prerendered page runs on the device", "host", SERVER,
     '    plan["prerender"] == true && plan.get("layout").is_none_or(|l| l.is_null())\n',
     '    plan.get("layout").is_none_or(|l| l.is_null())\n'),
    ("a page with a layout runs on the device", "host", SERVER,
     '    plan["prerender"] == true && plan.get("layout").is_none_or(|l| l.is_null())\n',
     '    plan["prerender"] == true\n'),
    ("no page runs on the device", "host", SERVER,
     "            let document = if runs_on_device(plan) {\n", "            let document = if false && runs_on_device(plan) {\n"),
    ("each marked page runs on the device", "host", SERVER,
     "            let document = if runs_on_device(plan) {\n", "            let document = if true || runs_on_device(plan) {\n"),
    ("a build that does not say so has one", "host", SERVER,
     '            device: delivery_says(dist, "device") && dist.join("pw-runtime-device.mjs").is_file(),\n',
     '            device: dist.join("pw-runtime-device.mjs").is_file(),\n'),
    ("a build with no file has one", "host", SERVER,
     '            device: delivery_says(dist, "device") && dist.join("pw-runtime-device.mjs").is_file(),\n',
     '            device: delivery_says(dist, "device"),\n'),
    ("the document starts the whole runtime", "host", SERVER,
     "            .replacen(&full, &device, 1)\n", "            .replacen(&full, &full, 1)\n"),
    ("the head preloads the whole runtime", "host", SERVER,
     '        let mut head = preload("/pw-runtime-device.mjs");\n',
     '        let mut head = preload("/pw-runtime.mjs");\n'),
    ("the head does not fetch the page's code", "host", SERVER,
     "            head.push_str(&preload(bundle));\n", "            let _ = (&preload, bundle);\n"),
    ("a page's code is named nowhere", "host", SERVER,
     "                    .on_device(document, self.bundles.contains(page).then_some(bundle.as_str()))\n",
     "                    .on_device(document, None::<&str>.or(Some(bundle.as_str()).filter(|_| false)))\n"),
    ("a page with no module names one", "host", SERVER,
     "                    .on_device(document, self.bundles.contains(page).then_some(bundle.as_str()))\n",
     "                    .on_device(document, Some(bundle.as_str()))\n"),
    ("the policy allows the whole runtime alone", "host", SERVER,
     "        scripts.extend(device.as_deref());\n", ""),
    ("the policy allows the first of several", "host", CSP,
     '            scripts\n                .iter()\n                .map(|s| sha256_source(s))\n',
     '            scripts\n                .iter()\n                .take(1)\n                .map(|s| sha256_source(s))\n'),
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


def node_tests(*files):
    out, code = bounded(["node", "--test", "--test-reporter=spec", *files], cwd=ROOT)
    if code is None:
        return True, 0, 1
    passed = sum(int(n) for n in re.findall(r"(?m)^ℹ pass (\d+)$", out))
    failed = sum(int(n) for n in re.findall(r"(?m)^ℹ fail (\d+)$", out))
    return passed + failed > 0, passed, failed


def contract():
    return node_tests("spikes/own-renderer/renderer-contract.test.mjs")


def decision():
    return node_tests("spikes/own-renderer/decision-agreement.test.mjs")


def packaging():
    return node_tests("spikes/own-renderer/optimize-assets.test.mjs", "spikes/own-renderer/host-plan.test.mjs")


def host():
    total = (0, 0)
    for name in ("tests::page_bundle::", "tests::todomvc::a_production_build_of_the_page"):
        out, code = bounded(["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server", name], cwd=ROOT)
        if code is None:
            return True, 0, 1
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not found:
            return False, 0, 0
        total = (total[0] + sum(int(p) for p, _ in found), total[1] + sum(int(f) for _, f in found))
    return True, total[0], total[1]


def browser():
    env = {**os.environ, "PW_PRODUCTION_ASSETS": "1"}
    _, code = bounded(["bash", "spikes/own-renderer/todomvc.sh"], cwd=ROOT, env=env)
    if code != 0:
        return False, 0, 0
    out, code = bounded(["pnpm", "exec", "playwright", "test", "e2e/device-runtime.spec.mjs", "--project=chromium", "--reporter=line"],
                        cwd=ROOT / "spikes/own-renderer", env=env)
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


def nested():
    """The nested blocks' page, on a production build of the demos."""
    env = {**os.environ, "PW_PRODUCTION_ASSETS": "1", "BUILD_ONLY": "1"}
    _, code = bounded(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT, env=env)
    if code != 0:
        return False, 0, 0
    env = {**os.environ, "PW_PRODUCTION_ASSETS": "1"}
    out, code = bounded(["pnpm", "exec", "playwright", "test", "e2e/nested-blocks.spec.mjs", "--project=chromium", "--reporter=line"],
                        cwd=ROOT / "spikes/own-renderer", env=env)
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {"decision": decision, "contract": contract, "packaging": packaging, "host": host, "browser": browser, "nested": nested}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite, run in SUITES.items():
        built, passed, failed = run()
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
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
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = SUITES[suite]()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)
    # What was built from a mutant is built again from the restored source.
    browser()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
