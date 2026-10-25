#!/usr/bin/env python3
"""**The source a developer writes, counted the same way in every stack**
(the head-to-head, ADR-XXXX; ADR-0342 §4's "lines of code").

    python3 scripts/head_to_head_loc.py [--json]

For each app (TodoMVC, the store) and each stack, the files are sorted into:

- **app**: the code a developer of that app writes in that stack: its pages,
  components, data and styles;
- **host**: code the app needs that is written in another language inside
  the stack's server rather than in the app (the Pleris store's data
  operations are Rust in the development server: `host "store:data/..."`);
  counted, never hidden;
- **build**: the app's own build and configuration files (package manifests,
  bundler and compiler configuration, build scripts);
- **excluded**: listed and not counted: lockfiles, committed builds,
  readmes.

**The rule**, printed with every count: a line counts when it holds anything
but white space and comments. Comments are a language's own: `//` and
`/* */` in JavaScript, TypeScript, CSS (`/* */` only) and Rust (whose block
comments nest, and whose test modules, a top-level `#[cfg(test)]` before a
`mod`, are left out); `//` and `<!-- -->` in Pleris; `<!-- -->` in HTML; a
Svelte component's `<script>` and `<style>` by their own languages and the
rest as HTML; `#` lines in shell. Strings are read as strings, so a `//`
inside one is code. Approximations, stated: a JavaScript regular expression
literal is read as code (one holding `//` or `/*` unescaped would be
misread), and a template literal's `${}` is read as part of the string.
"""

import fnmatch
import json
import pathlib
import sys

sys.dont_write_bytecode = True

ROOT = pathlib.Path(__file__).resolve().parent.parent

RULE = (
    "a line counts when it holds anything but white space and comments "
    "(each language's own comments; Rust test modules left out); "
    "lockfiles, committed builds and readmes are listed and not counted"
)

LANGUAGES = {
    ".pw": "Pleris",
    ".rs": "Rust",
    ".js": "JavaScript",
    ".mjs": "JavaScript",
    ".cjs": "JavaScript",
    ".jsx": "JavaScript (JSX)",
    ".ts": "TypeScript",
    ".tsx": "TypeScript (TSX)",
    ".svelte": "Svelte",
    ".css": "CSS",
    ".html": "HTML",
    ".sh": "Bash",
    ".json": "JSON",
    "": "JSON",  # .eslintrc
}

TODOMVC = "benchmarks/headtohead/todomvc"
NEXT = "benchmarks/baselines/next-react"
SVELTEKIT = "benchmarks/baselines/sveltekit"

# What each stack's developer writes, by app. Globs are relative to the root.
SETS = {
    "todomvc": {
        "pleris": {
            "app": ["examples/todomvc/app.pw"],
            "host": [],
            "build": ["spikes/own-renderer/todomvc.sh", "spikes/own-renderer/todomvc-styles.mjs"],
            "excluded": [],
        },
        "react": {
            "app": [f"{TODOMVC}/react/src/**", f"{TODOMVC}/react/public/index.html"],
            "host": [],
            "build": [
                f"{TODOMVC}/react/package.json",
                f"{TODOMVC}/react/webpack.*.js",
                f"{TODOMVC}/react/.eslintrc",
            ],
            "excluded": [
                f"{TODOMVC}/react/package-lock.json",
                f"{TODOMVC}/react/dist/**",
                f"{TODOMVC}/react/readme.md",
                f"{TODOMVC}/react/.gitignore",
            ],
        },
        "svelte": {
            "app": [f"{TODOMVC}/svelte/src/**", f"{TODOMVC}/svelte/index.html"],
            "host": [],
            "build": [f"{TODOMVC}/svelte/package.json", f"{TODOMVC}/svelte/vite.config.js"],
            "excluded": [
                f"{TODOMVC}/svelte/package-lock.json",
                f"{TODOMVC}/svelte/dist/**",
                f"{TODOMVC}/svelte/readme.md",
                f"{TODOMVC}/svelte/.gitignore",
            ],
        },
    },
    "store": {
        "pleris": {
            "app": ["benchmarks/baselines/pleris/**.pw"],
            # The store's data operations (`host "store:data/..."`): Rust in the
            # development server, shared by every store example and task.
            "host": ["spikes/own-renderer/server/src/store.rs"],
            "build": ["spikes/own-renderer/baseline-store.sh"],
            "excluded": [],
        },
        "next-react": {
            "app": [f"{NEXT}/app/**", f"{NEXT}/lib/**", f"{NEXT}/proxy.ts"],
            "host": [],
            "build": [f"{NEXT}/package.json", f"{NEXT}/next.config.mjs", f"{NEXT}/tsconfig.json"],
            "excluded": [],
        },
        "sveltekit": {
            "app": [f"{SVELTEKIT}/src/**"],
            "host": [],
            "build": [
                f"{SVELTEKIT}/package.json",
                f"{SVELTEKIT}/svelte.config.js",
                f"{SVELTEKIT}/vite.config.ts",
                f"{SVELTEKIT}/tsconfig.json",
            ],
            "excluded": [],
        },
    },
}


def strip_c_like(text, line_comments=True, nested=False, quotes="\"'`"):
    """The text with its comments blanked, line breaks kept: `//` to the end
    of a line (when the language has them) and `/* */` (nested in Rust).
    Strings in `quotes` are kept whole, escapes and all."""
    out = []
    i, n = 0, len(text)
    depth = 0
    quote = None
    while i < n:
        c = text[i]
        if depth:
            if nested and text.startswith("/*", i):
                depth += 1
                i += 2
            elif text.startswith("*/", i):
                depth -= 1
                i += 2
            else:
                out.append("\n" if c == "\n" else " ")
                i += 1
            continue
        if quote:
            out.append(c)
            if c == "\\" and i + 1 < n:
                out.append(text[i + 1])
                i += 2
                continue
            if c == quote:
                quote = None
            elif c == "\n" and quote != "`" and quote != '"':
                quote = None
            i += 1
            continue
        if line_comments and text.startswith("//", i):
            while i < n and text[i] != "\n":
                i += 1
            continue
        if text.startswith("/*", i):
            depth = 1
            i += 2
            continue
        if c in quotes:
            quote = c
        out.append(c)
        i += 1
    return "".join(out)


def strip_markup(text):
    """The text with its `<!-- -->` comments blanked, line breaks kept."""
    out = []
    i, n = 0, len(text)
    while i < n:
        if text.startswith("<!--", i):
            end = text.find("-->", i + 4)
            end = n if end < 0 else end + 3
            out.append("".join("\n" if ch == "\n" else " " for ch in text[i:end]))
            i = end
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def without_rust_tests(text):
    """A Rust file without its test modules: a top-level `#[cfg(test)]`
    directly before a `mod name {`, to that module's closing brace at the
    start of a line."""
    lines = text.split("\n")
    out, skipping = [], False
    i = 0
    while i < len(lines):
        line = lines[i]
        if not skipping and line.strip() == "#[cfg(test)]" and i + 1 < len(lines) and lines[i + 1].startswith("mod "):
            skipping = True
            out.append("")
            i += 1
            continue
        if skipping:
            out.append("")
            if line == "}":
                skipping = False
            i += 1
            continue
        out.append(line)
        i += 1
    return "\n".join(out)


def svelte_parts(text):
    """A component's text with each part's comments blanked by its own
    language: `<script>` as TypeScript/JavaScript, `<style>` as CSS, the
    rest as markup."""
    out = []
    i = 0
    while i < len(text):
        start_script = text.find("<script", i)
        start_style = text.find("<style", i)
        starts = [s for s in (start_script, start_style) if s >= 0]
        if not starts:
            out.append(strip_markup(text[i:]))
            break
        s = min(starts)
        out.append(strip_markup(text[i:s]))
        tag = "script" if s == start_script else "style"
        open_end = text.find(">", s) + 1
        close = text.find(f"</{tag}>", open_end)
        close = len(text) if close < 0 else close
        out.append(text[s:open_end])
        body = text[open_end:close]
        out.append(strip_c_like(body, line_comments=(tag == "script")))
        i = close
    return "".join(out)


def code_lines(path, text):
    """The lines of a file that hold code, by its language's rule."""
    ext = path.suffix
    if ext == ".rs":
        stripped = strip_c_like(without_rust_tests(text), nested=True, quotes='"')
    elif ext in (".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx"):
        stripped = strip_c_like(text)
    elif ext == ".css":
        stripped = strip_c_like(text, line_comments=False, quotes="\"'")
    elif ext == ".pw":
        stripped = strip_markup(strip_c_like(text, quotes='"'))
    elif ext == ".html":
        stripped = strip_markup(text)
    elif ext == ".svelte":
        stripped = svelte_parts(text)
    elif ext == ".sh":
        stripped = "\n".join("" if l.lstrip().startswith("#") else l for l in text.split("\n"))
    else:
        stripped = text
    return sum(1 for line in stripped.split("\n") if line.strip())


def expand(patterns):
    """Files the globs name, each once, in order. A glob's `*` crosses
    directories (`fnmatch`), and is searched for under the directory its
    pattern names before its first wildcard, `node_modules` aside."""
    out = []
    for pattern in patterns:
        if any(ch in pattern for ch in "*?["):
            parts = pattern.split("/")
            fixed = []
            for part in parts:
                if any(ch in part for ch in "*?["):
                    break
                fixed.append(part)
            base = ROOT.joinpath(*fixed)
            found = sorted(
                p for p in base.rglob("*")
                if p.is_file() and "node_modules" not in p.relative_to(ROOT).parts
            )
            for p in found:
                rel = p.relative_to(ROOT).as_posix()
                if fnmatch.fnmatch(rel, pattern) and p not in out:
                    out.append(p)
        else:
            p = ROOT / pattern
            if not p.is_file():
                raise SystemExit(f"head_to_head_loc: {pattern} is not a file")
            if p not in out:
                out.append(p)
    return out


def count_set(spec):
    result = {}
    for part in ("app", "host", "build"):
        files = []
        for p in expand(spec[part]):
            lines = code_lines(p, p.read_text(encoding="utf-8"))
            files.append({"path": p.relative_to(ROOT).as_posix(), "language": LANGUAGES.get(p.suffix, p.suffix), "lines": lines})
        by_language = {}
        for f in files:
            by_language[f["language"]] = by_language.get(f["language"], 0) + f["lines"]
        result[part] = {
            "files": files,
            "lines": sum(f["lines"] for f in files),
            "languages": by_language,
        }
    result["excluded"] = [p.relative_to(ROOT).as_posix() for p in expand(spec["excluded"])]
    return result


def count():
    return {app: {stack: count_set(spec) for stack, spec in stacks.items()} for app, stacks in SETS.items()}


def report(counts):
    lines = [f"rule: {RULE}", ""]
    for app, stacks in counts.items():
        lines.append(f"-- {app}")
        lines.append(f"   {'stack':<11} {'app lines':>9} {'files':>5}  {'languages (app)':<44} {'host':>6}  {'build/config':>12}")
        for stack, c in stacks.items():
            langs = ", ".join(f"{k} {v}" for k, v in sorted(c["app"]["languages"].items()))
            host = c["host"]["lines"]
            lines.append(
                f"   {stack:<11} {c['app']['lines']:>9} {len(c['app']['files']):>5}  {langs:<44} {host:>6}  "
                f"{c['build']['lines']:>5} in {len(c['build']['files'])} file(s)"
            )
        for stack, c in stacks.items():
            for part in ("app", "host", "build"):
                for f in c[part]["files"]:
                    lines.append(f"     {stack:<11} {part:<5} {f['lines']:>5}  {f['language']:<17} {f['path']}")
            for x in c["excluded"]:
                lines.append(f"     {stack:<11} not counted  {x}")
        lines.append("")
    return "\n".join(lines)


def main(argv):
    counts = count()
    if "--json" in argv:
        print(json.dumps({"rule": RULE, "counts": counts}, indent=2))
    else:
        print(report(counts))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
