#!/usr/bin/env python3
"""Plan a verification run (ADR-0245): evidence recipes, in shards.

The recipes are `just`'s own list, kept where they record evidence
(`e<N>-…`) and are not measurements of the machine that records them
(`LOCAL_ONLY`). Which of them:
- every one (`--all`), as the nightly run plans;
- those a range of commits touches (`--changed BASE HEAD`), as a push plans,
  the chain's choice: each recipe running a mutation script that the range
  changed, whose mutant sits within `--near` lines of a changed line, or
  whose tests the range changed, a Rust test or a browser spec (ADR-0281);
  each recipe that runs a browser spec the range changed; each recipe whose
  own script changed or reads a program or a benchmark the range changed
  (`data_inputs`), whose own lines name one the range changed
  (`body_inputs`), or that runs a shell script the range changed; and each
  recipe whose own lines changed; and with `--current`, as a push plans,
  less each recipe whose committed evidence is current at HEAD (`current`,
  ADR-0281, amended 2026-10-13 (6)); and with `--stale`, as a train's push
  and master's plan, each recipe whose committed evidence is stale at HEAD,
  whatever the range (`stale`, track evidence-integrity);
- or those named.

Each is dealt, the longest first, to the shard where it ends soonest
(ADR-0290): its seconds, as the last run that ran it to its end took them
(`SECONDS`), or, not yet measured, the mutants its scripts plant at the
measured median; and what it adds to that shard's setup. A shard installs
browsers, or builds, only where a recipe dealt it needs them (ADR-0249),
and a recipe that needs less may run where more is set up. The recipes
that need a database (`NEEDS_DATABASE`) run in shards of their own, each
given a PostgreSQL of its own (ADR-0278), as many shards as make the run
end soonest; the rest are dealt into the shards left.
Prints, for `$GITHUB_OUTPUT`, `shards=<json>`, a list of `{"index": i,
"recipes": [...], "browsers": b, "build": b, "database": b}`, empty where
nothing is to run.

    python3 scripts/ci_plan.py --shards 17 --all
    python3 scripts/ci_plan.py --shards 17 --changed BASE HEAD [--current] [--stale]
    python3 scripts/ci_plan.py --shards 17 e14-feed e14-read-whole
"""

import argparse
import fnmatch
import importlib.util
from collections.abc import Callable
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Each recipe's seconds in the last run that ran it to its end, written by
# `evidence_fetch.py` from each shard's `results.json` (ADR-0290).
SECONDS = ROOT / "scripts/ci_seconds.json"

# What a recipe not yet measured takes for itself and each mutant it
# plants, in seconds, where no recipe is measured: the median of the
# nightly of 2026-10-08, 213 recipes.
RATE = 24

# What a shard sets up before its first recipe, in seconds, as that nightly
# took it: a plain shard 0.6 to 1.0 minutes; one that builds, or installs
# browsers and builds, 2.5 to 6.5.
SETUP = {(False, False): 42, (False, True): 384, (True, False): 390, (True, True): 390}

# **What a shard is planned to end within**, in seconds (ADR-0290, amended
# 2026-10-10). A shard runs at most 345 minutes (verify.yml's
# `timeout-minutes`, 20,700 s), and one stopped there is a recipe shard that
# failed: nothing of its run is fetched (ADR-0281), and the run is made again
# whole. Half the limit: a branch's recipes run longer than their recorded
# seconds until its own run measures them, up to twice: track/command-order's
# verify 38015617597 dealt 13,959 s into its shard 7, whose two recipes then
# took twice theirs, and track/store-accounts' 38046546700 ran its shard 6's
# 1.6 to 1.7 times (its server's tests grew, and each mutant runs them
# whole); each was stopped at 345 minutes. Two thirds (13,800 s, 2026-10-10)
# was too little. A run is dealt into more shards than it asks for where its
# number would end past this, up to `MOST`: the jobs past the Free plan's 20
# wait for one to end, which costs a run less than a stopped shard does.
CEILING = 10_350
MOST = 40

# **A job's limit**, in seconds: verify.yml's `timeout-minutes`, 345. A
# recipe recorded past it, with the setup of a shard that runs it, runs
# whole in no shard: train-10's verify 39054436657 stopped `e14-refusal`,
# alone in its shard, at the limit after 20,507 s, five and a half hours in,
# and nothing of it was kept. So the plan refuses such a recipe before any
# shard runs (`past_the_limit`), and its mutants are split across recipes
# first (the integrator's ruling 2 of 2026-10-17).
LIMIT = 20_700

# Measurements of the machine they run on: a time, a load, a memory curve,
# each naming its machine in a `host:` line. A shared runner's numbers are
# not comparable from one run to the next, so these are recorded on the
# development machine and never fetched from a run.
LOCAL_ONLY = {
    "e7-performance",
    "e9-latency",
    "e10-bench",
    "e10-close-bench",
    "e10-load",
    "e10-memory",
    # The kiokun track's (docs/PARALLEL.md, W6's plan; ADR-0281's fifth
    # decision): it reads the owner's kiokun-data checkout, which no runner
    # has. What needs the owner's local data is recorded here; what a
    # committed sample shows runs on CI.
    "e14-kiokun-inventory",
    # The kiokun track's sample of the whole dictionary: the owner's data, and
    # this machine's times.
    "e14-kiokun-sample",
    # And its page head against kiokun.com's own code, copied from the
    # owner's checkout.
    "e14-kiokun-seo",
    # And its Japanese examples against kiokun.com's own code.
    "e14-kiokun-examples",
    # And its moves against kiokun.com's own code.
    "e14-kiokun-moves",
    # And its Contains and Appears in against kiokun.com's own code.
    "e14-kiokun-contains",
    # And its search against kiokun.com's own code and index.
    "e14-kiokun-search",
    # And its Homophones against kiokun.com's own reading index.
    "e14-kiokun-homophones",
    # And its deinflection against kiokun.com's own resolution.
    "e14-kiokun-deinflection",
    # The benchmark harness's controls over a task's three stacks: each
    # sandbox installs its stack's dependencies from this machine's pnpm
    # store, offline by design (`bench.mjs`'s `sandbox`), and a runner's
    # store holds no Next.js or SvelteKit. Planned on CI for the first time
    # on train-10 (the planner reads a recipe with parameters since track
    # evidence-integrity-2, which also stopped its `|| true`), it failed
    # there at its first sandbox (verify 39054436657). Its Pleris stack runs
    # on CI as `e14-pleris-controls`.
    "e14-harness",
}

# Recipes run against a database (ADR-0246). Each is a shard of its own,
# beside a PostgreSQL of its own (ADR-0278), set up as any shard is for its
# recipe (ADR-0258): `e14-identity` and `e14-uploads` (ADR-0260) drive
# browsers.
NEEDS_DATABASE = {
    "e14-feed-postgres",
    "e14-identity",
    "e14-uploads",
    "e14-notifications",
    "e14-messages",
    "e14-store-postgres",
    "e14-store-accounts",
    "e14-store-search",
    "e14-store-hours",
    "e14-order-total",
    "e14-bounded-statements",
    "e14-render-merge",
    "e14-interactions",
    # NEXT 6c: the store's orders awaiting their payments, on PostgreSQL too.
    "e14-payments",
    # The scheduled-commands track: a run kept in each layer's database.
    "e14-scheduled",
    "e14-scheduled-replay",
    "e14-session-command-authority",
    "e14-session-receipt-recovery",
    # Track `reviews`: a store's ratings, on PostgreSQL too.
    "e14-reviews",
    # Track `scheduled-orders` (ADR-0347): without a database it says it
    # skipped and writes no evidence, and train-10's verify (39054436657)
    # dealt it where there was none, so the run recorded nothing of it.
    "e14-scheduled-orders",
    # Track `argument-invariants` (ADR-0346): a row on PostgreSQL held to
    # its type's invariant, run where a database is named.
    "e14-argument-invariants",
    # Track `second-host`: hosts that share one database.
    "e14-second-host",
}

EVIDENCE_RECIPE = re.compile(r"^e[0-9]+-[a-z0-9-]+$")

# How near a changed line a mutant's anchor plans its script (`--near`):
# a push's plan and a fetch's check of what changed since its run read the
# same.
NEAR = 30


def recipes() -> list[str]:
    """Every recipe `just` knows, by name."""
    out = subprocess.run(
        ["just", "--summary"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout
    return out.split()


IMPORT = re.compile(r"""^import\??\s+(['"])(.+?)\1""")

# **A recipe's header**: its name, its parameters, each with its default
# where it has one, and its colon (not `:=`, an assignment's). Until
# 2026-10-15 a parameter's default stopped it (`[^:=]*` before the colon),
# so `e10-browser engines="..." out="..."`, `e10-close-bench RUNS="8"` and
# `e14-harness TASK="T08"` had no body here: no change planned them by what
# they run, and a shard dealt one set up neither browsers nor the build
# (track evidence-integrity-2).
HEADER = re.compile(
    r"""^([a-z0-9][a-z0-9-]*)"""
    r"""(?:\s+[$*+]?[A-Za-z_][A-Za-z0-9_-]*(?:=(?:"[^"]*"|'[^']*'|[^\s'":=]+))?)*"""
    r"""\s*:(?!=)"""
)


def justfile_parts(root: pathlib.Path = ROOT) -> list[str]:
    """The justfile and each file it imports (ADR-0253), each by its path
    from the repository: a parallel track's recipes live in a file of its
    own, which `just` reads as part of the justfile."""
    parts = ["justfile"]
    for line in (root / "justfile").read_text().splitlines():
        m = IMPORT.match(line)
        if m and (root / m.group(2)).is_file():
            parts.append(m.group(2))
    return parts


def bodies(root: pathlib.Path = ROOT) -> dict[str, str]:
    """Each recipe's body, by name, in every part of the justfile."""
    out: dict[str, str] = {}
    for part in justfile_parts(root):
        name = None
        for line in (root / part).read_text().splitlines():
            m = HEADER.match(line)
            if m and not line.startswith((" ", "\t")):
                name = m.group(1)
                out[name] = ""
                continue
            if name and (line.startswith((" ", "\t")) or not line.strip()):
                out[name] += line + "\n"
            else:
                name = None
    return out


def recipes_at(text: list[str], lines: list[tuple[int, int]], names: list[str]) -> set[str]:
    """The recipes of `names` whose own lines are among `lines`, in a part of
    the justfile: for each, the nearest header above it."""
    out = set()
    for a, b in lines:
        for line in range(a, b + 1):
            for k in range(min(line, len(text)) - 1, -1, -1):
                m = HEADER.match(text[k])
                if m and not text[k].startswith((" ", "\t")):
                    if m.group(1) in names:
                        out.add(m.group(1))
                    break
                if text[k].strip() and not text[k].startswith((" ", "\t", "#")):
                    break
    return out


def mutants(script: str) -> int:
    """How many mutants `scripts/<script>` plants, or 0 where it cannot be read."""
    path = ROOT / "scripts" / script
    try:
        spec = importlib.util.spec_from_file_location(path.stem, path)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
        return len(getattr(module, "MUTANTS", []))
    except Exception:
        return 0


def cost(body: str) -> int:
    """A recipe's work: one for itself, and one for each mutant it plants."""
    scripts = set(re.findall(r"scripts/([a-z0-9_]+_mutations\.py)", body))
    return 1 + sum(mutants(s) for s in scripts)


def hunks(base: str, head: str | None, root: pathlib.Path | None = None) -> dict[str, list[tuple[int, int]]]:
    """Each file the range changed, with its changed lines' ranges in `head`
    (a deletion is the line it stood before). `head` None is the tree here,
    what is staged and what is not: a merge not yet committed
    (`evidence_fetch.moved_on`). A binary file changed is a change, too, of
    each line that reads it (`with_readers`). `root` is the repository's
    where none is given, as it is at the call."""
    root = ROOT if root is None else root
    # Bytes, and paths as written: a file git diffs as text need not be
    # UTF-8, and a path outside ASCII is quoted unless told not to be.
    diff = subprocess.run(
        ["git", "-c", "core.quotePath=false", "diff", "-U0", base, *([head] if head else [])],
        cwd=root,
        capture_output=True,
        check=True,
    ).stdout
    return with_readers(parsed(diff), binaries(diff), head, root)


def parsed(diff: bytes) -> dict[str, list[tuple[int, int]]]:
    """**A diff's files and their changed lines** (`hunks`). Read as UTF-8
    with each byte that is not one replaced: until 2026-10-12 it was decoded
    strictly, and a raw DEFLATE file that held no NUL byte, which git diffs
    as text, stopped a track's plan (W6's `吐く.json.deflate`), so its verify
    ran no recipe. Only its paths and its hunks' line numbers are read, and
    those are ASCII where a file's own bytes are not."""
    diff = diff.decode("utf-8", errors="replace")
    out: dict[str, list[tuple[int, int]]] = {}
    current = None
    for line in diff.splitlines():
        if line.startswith("+++ "):
            current = None if line == "+++ /dev/null" else line[len("+++ b/"):]
            if current:
                out.setdefault(current, [])
            continue
        m = re.match(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
        if m and current:
            start, count = int(m.group(1)), int(m.group(2) or 1)
            out[current].append((start, start + max(count, 1) - 1))
    return out


# **A binary file's change is planned by what reads it** (train-11,
# 2026-10-20, the integrator's addition to its committed components). git
# diffs a binary file with no `+++` line and no hunk, only `Binary files a/x
# and b/x differ`, so until now it changed nothing here: train-10 regenerated
# three of the store's components (8c5618c8) and no recipe was planned, nor
# any evidence staled, and the 72 recipes that read them were chosen by hand.
# A binary is read where a text file names it, so each line that does is a
# changed line of its file, and the rules above plan from there: the recipe
# whose own lines name it, a test file that loads it (and so the scripts and
# recipes that run that test), a mutant beside the line.
BINARY = re.compile(r"^Binary files (?:a/(.+?)|/dev/null) and (?:b/(.+?)|/dev/null) differ$", re.M)

# Records, not readers: what names a binary under them reads nothing
# (`verified_base.RECORDS`, the same two).
RECORDS = ("docs/", "scripts/ci_seconds.json")


def binaries(diff: bytes) -> list[str]:
    """The files a diff calls binary: each one's path after the change, or
    before it where the change deleted the file."""
    text = diff.decode("utf-8", errors="replace")
    return sorted({m.group(2) or m.group(1) for m in BINARY.finditer(text)})


def names_binary(text: str, directory: str, name: str) -> bool:
    """Whether a line names the file `directory/name`: by its path; by its
    directory as a whole (`root.join("<directory>")`, `cp ... <directory>/`);
    or by its directory and a name a glob or a placeholder makes it
    (`<directory>/*.wasm`, `format!("<directory>/{id}.wasm")`,
    `<directory>/${id}.wasm`). Never by another file's name or a directory
    beside it (`<directory>/handlers`, `<directory>0`)."""
    for m in re.finditer(r"(?<![\w.-])" + re.escape(directory) + r"(?![\w.-])(/[-\w.*?{}$%\[\]/]*)?", text):
        rest = (m.group(1) or "/")[1:]
        if not rest:
            return True
        if "/" in rest:
            continue
        pattern = re.sub(r"\$\{[^}]*\}|\$\w+|\{[^}]*\}|%[sd]", "*", rest)
        if fnmatch.fnmatchcase(name, pattern):
            return True
    return False


def read_by(
    binary: str, head: str | None, root: pathlib.Path | None = None
) -> dict[str, list[tuple[int, int]]]:
    """The lines of the text files `head` holds (the tree here where it is
    None) that name `binary` (`names_binary`), records aside, by file: each
    a line range of one line."""
    root = ROOT if root is None else root
    directory, _, name = binary.rpartition("/")
    if not directory:
        return {}
    command = ["git", "-c", "core.quotePath=false", "grep", "-n", "-I", "-F", "-e", directory]
    if head:
        command.append(head)
    found = subprocess.run(command + ["--"], cwd=root, capture_output=True).stdout.decode(
        "utf-8", errors="replace"
    )
    out: dict[str, list[tuple[int, int]]] = {}
    prefix = f"{head}:" if head else ""
    for line in found.splitlines():
        if not line.startswith(prefix):
            continue
        path, number, text = (line[len(prefix):].split(":", 2) + ["", ""])[:3]
        if not number.isdigit() or path == binary or path.startswith(RECORDS):
            continue
        if names_binary(text, directory, name):
            out.setdefault(path, []).append((int(number), int(number)))
    return out


def with_readers(
    changed: dict[str, list[tuple[int, int]]],
    changed_binaries: list[str],
    head: str | None,
    root: pathlib.Path | None = None,
) -> dict[str, list[tuple[int, int]]]:
    """`changed`, each binary in `changed_binaries` among its files, and the
    lines that read it (`read_by`) among their files' changed lines."""
    out = {path: list(lines) for path, lines in changed.items()}
    for binary in changed_binaries:
        out.setdefault(binary, [])
        for path, lines in read_by(binary, head, root).items():
            out.setdefault(path, []).extend(lines)
    return out


def changed_specs(changed: dict[str, list[tuple[int, int]]], root: pathlib.Path | None = None) -> set[str]:
    """The browser specs a change reaches, by file name: `pages.spec.mjs`;
    and each spec that imports a module of its own directory the change
    reaches (`e2e/accessibility-rules.mjs`, which three specs import: until
    2026-10-15 a change to it planned nothing, track evidence-integrity-2)."""
    root = root or ROOT
    out = {pathlib.Path(p).name for p in changed if p.endswith(".spec.mjs")}
    for p in changed:
        path = pathlib.Path(p)
        if p.endswith(".spec.mjs") or path.suffix != ".mjs" or not (root / path.parent).is_dir():
            continue
        for spec in sorted((root / path.parent).glob("*.spec.mjs")):
            try:
                text = spec.read_text()
            except OSError:
                continue
            if re.search(rf"""from\s+["']\./{re.escape(path.name)}["']""", text):
                out.add(spec.name)
    return out


# **The browser suite's configuration** (the integrator's ruling 7,
# 2026-10-15): every spec run from its directory runs under it, its hosts and
# its checks of the builds, so a change to it plans each recipe that runs a
# suite there, as a changed spec plans those that run it (ADR-0281). Until
# then a change to it planned only a recipe whose mutant's anchor sat near
# the change (track evidence-integrity's finding). The benchmark harness's
# is under `benchmarks/`, whose changes plan each recipe that names it
# (`body_inputs`).
SUITE_DIRECTORY = "spikes/own-renderer"
PLAYWRIGHT_CONFIG = f"{SUITE_DIRECTORY}/playwright.config.mjs"


def suite_configuration(root: pathlib.Path | None = None) -> set[str]:
    """The suite's configuration and every module of its directory it reads,
    as far as they go: what it imports (`from "./host-plan.mjs"`) and what
    those name beside themselves (`new URL("./host-registry.json",
    import.meta.url)`). Since the host map (ADR-0356) the configuration plans
    each run's hosts with `host-metadata.mjs`, `host-plan.mjs`, its reporter
    and the registry, so a change to any of them is a change to every suite."""
    root = root or ROOT
    out: set[str] = set()
    pending = [PLAYWRIGHT_CONFIG]
    while pending:
        rel = pending.pop()
        if rel in out:
            continue
        out.add(rel)
        try:
            text = (root / rel).read_text()
        except OSError:
            continue
        here = pathlib.PurePosixPath(rel).parent
        for name in re.findall(r"""(?:\bfrom\s+|\bimport\(\s*|new URL\(\s*)["']\./([\w.-]+\.(?:mjs|js|json))["']""", text):
            pending.append(str(here / name))
    return out


def suite_runs(body: str) -> list[tuple[str, str]]:
    """Each browser suite a recipe's own lines run: (the directory it runs
    from, its command), by `scripts/run_suite.sh` (from `spikes/own-renderer`
    unless its `-C` names another directory) or after a `cd`."""
    out = []
    for line in body.replace("\\\n", " ").splitlines():
        for options, command in re.findall(r"run_suite\.sh((?:\s+(?:-C|--runs)\s+\S+)*)\s+--\s+([^;]*)", line):
            where = re.search(r"-C\s+(\S+)", options)
            out.append(((where.group(1) if where else SUITE_DIRECTORY).rstrip("/"), command))
        for directory, command in re.findall(r"cd\s+(\S+)\s+&&\s+([^;|)]*playwright\s+test[^;|)]*)", line):
            out.append((directory.rstrip("/"), command))
    return out


def runs_browser_suite(body: str, directory: str = SUITE_DIRECTORY) -> bool:
    """Whether a recipe runs a browser suite from `directory`, under its
    configuration: in its own lines (`suite_runs`), or by a mutation script
    it runs that runs Playwright and names the directory."""
    if any(d == directory for d, _ in suite_runs(body)):
        return True
    for script in sorted(set(re.findall(r"scripts/([a-z0-9_]+_mutations\.py)", body))):
        try:
            text = (ROOT / "scripts" / script).read_text()
        except OSError:
            continue
        if re.search(r'"playwright",\s*"test"|playwright\s+test', text) and re.search(
            rf"""["']{re.escape(directory)}["']""", text
        ):
            return True
    return False


def runs_every_spec(body: str, directory: str = SUITE_DIRECTORY) -> bool:
    """Whether a recipe's own lines run every spec of `directory`: a suite
    run there that names no spec (`e10-browser`'s three full runs)."""
    return any(d == directory and ".spec.mjs" not in command for d, command in suite_runs(body))


def test_modules(changed: dict[str, list[tuple[int, int]]]) -> set[str]:
    """The Rust sources whose own test module a change touches, by the
    module's name: a line after `#[cfg(test)] mod tests {` in `csp.rs` is
    `csp`'s, which a filter `csp::` runs. Train-6's survivor's test was
    one, and planned nothing: not a test file, and far from every
    mutant."""
    out = set()
    for p, hunks in changed.items():
        if not p.endswith(".rs") or "/tests/" in p or not (ROOT / p).is_file():
            continue
        lines = (ROOT / p).read_text().splitlines()
        start = next(
            (
                i + 1
                for i in range(1, len(lines))
                if lines[i].strip() == "mod tests {" and lines[i - 1].strip() == "#[cfg(test)]"
            ),
            None,
        )
        if start is not None and any(last >= start for _, last in hunks):
            out.add(pathlib.Path(p).stem)
    return out


def names_module(text: str, module: str) -> bool:
    """Whether `text` names `module`'s tests by its filter, `csp::`, and
    not as the tail of another path, `pw_core::csp::`."""
    return re.search(rf"(?<![\w:]){re.escape(module)}::", text) is not None


def root_test_packages(changed: dict[str, list[tuple[int, int]]]) -> set[str]:
    """Crate-root test modules have no `main::` or `lib::` namespace.
    Reach every consumer of their package's tests, including bare filters."""
    packages = set()
    for path, lines in changed.items():
        if pathlib.Path(path).name not in {"main.rs", "lib.rs"} or not test_modules({path: lines}):
            continue
        for directory in (ROOT / path).parents:
            manifest = directory / "Cargo.toml"
            if manifest.is_file():
                section = re.search(r"(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)", manifest.read_text())
                name = re.search(r"(?m)^name\s*=\s*['\"]([^'\"]+)['\"]", section[1]) if section else None
                if name:
                    packages.add(name[1])
                break
            if directory == ROOT:
                break
    return packages


def names_package_tests(text: str, packages: set[str]) -> bool:
    """Package selectors in shell commands and literal Python argument lists.
    A workspace test consumer is conservatively a consumer of every package."""
    if not packages or not re.search(r"\bcargo\b", text) or not re.search(r"\btest\b", text):
        return False
    if re.search(r"(?<![\w-])--workspace(?![\w-])", text):
        return True
    return any(
        re.search(rf"(?<![\w-])(?:-p|--package)(?:[\s'\",]+|=)['\"]?{re.escape(package)}(?![\w-])", text)
        for package in packages
    )


def names_python_tests(text: str, changed: dict[str, list[tuple[int, int]]]) -> bool:
    """Explicit Python test paths, unittest module selectors, or discovery names."""
    for path in changed:
        if not path.startswith("scripts/tests/") or not path.endswith(".py"):
            continue
        module = path[:-3].replace("/", ".")
        if re.search(rf"(?<![\w./-]){re.escape(path)}(?![\w./-])", text):
            return True
        if re.search(rf"(?<![\w.]){re.escape(module)}(?!\w)", text):
            return True
        directory, name = path.rsplit("/", 1)
        directory_named = re.search(rf"(?<![\w./-]){re.escape(directory)}(?![\w./-])", text)
        if directory_named and re.search(rf"(?<![\w./-]){re.escape(name)}(?![\w./-])", text):
            return True
        # Native producer inventories may append .py to a bare module later.
        if directory_named and re.search(rf"(?<![\w.]){re.escape(name[:-3])}(?!\w)", text):
            return True
    return False


def touched_scripts(changed: dict[str, list[tuple[int, int]]], near: int) -> set[str]:
    """The mutation scripts a change reaches: the script itself, a mutant
    within `near` lines of a changed line, or a test file it runs, a Rust
    test or a browser spec (ADR-0281): a test target by `--test`, or one of
    the server's test modules by its filter, `tests::store_scheduled::`,
    which planned nothing until train-6's fix to one."""
    tests = {pathlib.Path(p).stem for p in changed if "/tests/" in p and p.endswith(".rs")}
    modules = test_modules(changed)
    packages = root_test_packages(changed)
    specs = changed_specs(changed)
    out = set()
    for path in sorted((ROOT / "scripts").glob("*_mutations.py")):
        rel = str(path.relative_to(ROOT))
        if rel in changed:
            out.add(path.name)
            continue
        text = path.read_text()
        if any(
            f'"--test", "{t}"' in text or f"--test {t}" in text or f"tests::{t}::" in text
            for t in tests
        ):
            out.add(path.name)
            continue
        if names_package_tests(text, packages) or names_python_tests(text, changed):
            out.add(path.name)
            continue
        # A source's own test module, by its filter.
        if any(names_module(text, m) for m in modules):
            out.add(path.name)
            continue
        # A browser spec it runs: the mutants it plants are killed there.
        if any(s in text for s in specs):
            out.add(path.name)
            continue
        try:
            spec = importlib.util.spec_from_file_location(path.stem, path)
            module = importlib.util.module_from_spec(spec)
            assert spec.loader is not None
            spec.loader.exec_module(module)
        except Exception:
            continue
        for mutant in getattr(module, "MUTANTS", []):
            target = next((x for x in mutant if isinstance(x, pathlib.Path)), None)
            if target is None or len(mutant) < 2 or not isinstance(mutant[-2], str):
                continue
            rel_target = str(target.resolve().relative_to(ROOT)) if target.is_absolute() else str(target)
            if rel_target not in changed:
                continue
            source = (ROOT / rel_target).read_text()
            at = source.find(mutant[-2])
            if at < 0:
                out.add(path.name)
                break
            first = source.count("\n", 0, at) + 1
            last = first + mutant[-2].count("\n")
            if any(first - near <= b and a <= last + near for a, b in changed[rel_target]):
                out.add(path.name)
                break
    return out


# **Where a recipe's own script reads a program or a benchmark from**: the
# paths it names from the repository's root under these. A mutation script's
# are its mutants' targets, which `touched_scripts` reads by line instead.
DATA_ROOTS = ("benchmarks/", "examples/")


def data_inputs(script: pathlib.Path) -> list[str]:
    """The paths `script` names as `ROOT / "..."` under `DATA_ROOTS`:
    `semantic_diffs.py`'s `benchmarks/tasks` and `benchmarks/baselines/
    pleris`."""
    named = re.findall(r'ROOT\s*/\s*"([^"]+)"', script.read_text())
    return [p.rstrip("/") for p in named if p.startswith(DATA_ROOTS)]


def body_inputs(body: str) -> list[str]:
    """The programs and benchmarks a recipe's own lines name under
    `DATA_ROOTS`, a path or a glob: `e10-build`'s `examples/store/*.pw`."""
    return sorted(set(re.findall(r"(?<![\w./-])((?:examples|benchmarks)/[\w.*?/-]+)", body)))


def names_path(pattern: str, path: str) -> bool:
    """Whether a recipe's `pattern` names `path`: a glob, segment by
    segment, as a shell expands one (a `*` never crosses a `/`); a path, it
    or anything under it."""
    pattern = pattern.rstrip("/")
    if not any(ch in pattern for ch in "*?["):
        return path == pattern or path.startswith(pattern + "/")
    parts, names = pattern.split("/"), path.split("/")
    return len(parts) == len(names) and all(
        fnmatch.fnmatchcase(n, p) for p, n in zip(parts, names)
    )


def reads_what_changed(body: str, changed: dict[str, list[tuple[int, int]]]) -> bool:
    """Whether a recipe's own script, one that plants no mutant, changed or
    reads what changed. Until 2026-10-12 neither planned it: holds' merge
    changed the benchmark's baseline store, T11's patches stopped applying
    to it, and no verify planned `e14-diffs`, which applies them (W8's
    finding). And a shell script it runs, by its path: until 2026-10-13 a
    change to `spikes/wasmtime-component/audit.sh` planned no `e8-host`,
    whose script it is (train-3's run planned none). And a program its own
    lines name: until 2026-10-14 `e10-build`, which builds
    `examples/store/*.pw`, was planned by no push since 2026-10-12, and its
    evidence said `place_order` imports `user-orders#place` after ADR-0332
    moved it (train-5's finding)."""
    for rel in re.findall(r"(?<![\w./-])((?:[\w-]+/)+[\w.-]+\.sh)\b", body):
        if rel in changed:
            return True
    if any(names_path(p, c) for p in body_inputs(body) for c in changed):
        return True
    for name in re.findall(r"scripts/(\w+\.py)", body):
        if name.endswith("_mutations.py"):
            continue
        rel = f"scripts/{name}"
        if rel in changed:
            return True
        path = ROOT / rel
        if not path.exists():
            continue
        inputs = data_inputs(path)
        if any(c == i or c.startswith(i + "/") for i in inputs for c in changed):
            return True
    return False


def changed_recipes(base: str, head: str, near: int, names: list[str]) -> list[str]:
    """The recipes of `names` a range of commits touches (see the module)."""
    return recipes_for(hunks(base, head), near, names)


def recipes_for(changed: dict[str, list[tuple[int, int]]], near: int, names: list[str]) -> list[str]:
    """The recipes of `names` that `changed`, each file's changed lines,
    touches (see the module)."""
    scripts = touched_scripts(changed, near)
    body = bodies()
    specs = changed_specs(changed)
    tests = {pathlib.Path(p).stem for p in changed if "/tests/" in p and p.endswith(".rs")}
    modules = test_modules(changed)
    packages = root_test_packages(changed)
    every_spec = any(p.startswith(f"{SUITE_DIRECTORY}/e2e/") and p.endswith(".mjs") for p in changed)
    configuration = suite_configuration()
    out = {
        name
        for name in names
        if any(f"scripts/{s}" in body.get(name, "") for s in scripts)
        # A recipe that runs a browser spec the range changed (ADR-0281),
        # by its name, or every spec of the suite.
        or any(s in body.get(name, "") for s in specs)
        or (every_spec and runs_every_spec(body.get(name, "")))
        # A recipe that runs a suite under the configuration the range
        # changed (the integrator's ruling 7).
        or (bool(configuration & changed.keys()) and runs_browser_suite(body.get(name, "")))
        # And one that runs a server test module the range changed, by its
        # filter (train-6's finding).
        or any(f"tests::{t}::" in body.get(name, "") for t in tests)
        # Or a source's own test module, by its filter.
        or any(names_module(body.get(name, ""), m) for m in modules)
        or names_package_tests(body.get(name, ""), packages)
        or names_python_tests(body.get(name, ""), changed)
        # A recipe whose own script changed, or reads what changed.
        or reads_what_changed(body.get(name, ""), changed)
    }
    # A recipe whose own lines changed: written or changed in the range, in
    # any part of the justfile.
    for part in justfile_parts():
        if part in changed:
            text = (ROOT / part).read_text().splitlines()
            out |= recipes_at(text, changed[part], names)
    return sorted(out)


# **The commit an evidence file says it was recorded at** (ADR-0281,
# amended 2026-10-13 (6)): `commit: <sha>`, a run's whole commit. A file
# written from a tree with changes says `+ uncommitted changes` after it,
# and records no commit.
RECORDED_AT = re.compile(r"^commit: ([0-9a-f]{40})[ \t]*$", re.M)

# The evidence files a recipe writes directly or publishes after recording
# beside its destination (ADR-0344). Both feed the same discovery authority.
WRITES = re.compile(r">[ \t]*(docs/evidence/[\w./-]+\.txt)")
PUBLISHES = re.compile(r'\bmv[ \t]+(?:"[^"\n]+"|[^ \t\r\n]+)[ \t]+(docs/evidence/[\w./-]+\.txt)')


def evidence_files(body: str) -> list[str]:
    """The evidence files a recipe's lines write."""
    return sorted(set(WRITES.findall(body) + PUBLISHES.findall(body)))


def recorded_at(head: str, path: str, root: pathlib.Path = ROOT) -> str | None:
    """The commit `path`, as `head` holds it, was recorded at, or none."""
    out = subprocess.run(["git", "show", f"{head}:{path}"], cwd=root, capture_output=True)
    if out.returncode != 0:
        return None
    m = RECORDED_AT.search(out.stdout.decode("utf-8", errors="replace"))
    return m.group(1) if m else None


def descends(sha: str, head: str, root: pathlib.Path = ROOT) -> bool:
    """Whether `head` is `sha` or descends from it."""
    out = subprocess.run(["git", "merge-base", "--is-ancestor", sha, head], cwd=root, capture_output=True)
    return out.returncode == 0


def current(
    names: list[str],
    head: str,
    near: int = NEAR,
    body: dict[str, str] | None = None,
    read: Callable[[str, str], str | None] | None = None,
    descends_from: Callable[[str, str], bool] | None = None,
    reach: Callable[[str, str], set[str]] | None = None,
) -> dict[str, str]:
    """**The recipes of `names` whose committed evidence is current at
    `head`**, each with the commit it was recorded at (ADR-0281, amended
    2026-10-13 (6)): every file the recipe writes recorded at one whole
    commit that `head` descends from, and nothing the recipe reads changed
    since, by this planner's own rule (`reach`). A run passed it there: the
    fetch copies no file of a recipe that failed (`evidence_fetch.py`). A
    push does not run it again; the nightly runs it whatever its evidence
    says.
    Until then master's push after a train fixed by a dispatch planned the
    whole train again (verify 38494831583, cancelled by hand). Any error
    leaves a recipe planned."""
    body = bodies() if body is None else body
    read = read or recorded_at
    descends_from = descends_from or descends
    reach = reach or (lambda sha, h: set(recipes_for(hunks(sha, h), near, names)))
    out: dict[str, str] = {}
    reached: dict[str, set[str]] = {}
    for name in names:
        try:
            shas = {read(head, f) for f in evidence_files(body.get(name, ""))}
            # One commit for every file it writes, each file recorded.
            sha = next(iter(shas)) if len(shas) == 1 else None
            if sha is None:
                continue
            if not descends_from(sha, head):
                continue
            if sha not in reached:
                reached[sha] = reach(sha, head)
            if name not in reached[sha]:
                out[name] = sha
        except Exception as error:  # noqa: BLE001 - any failure plans it
            print(f"ci_plan: {name}'s evidence not read, so it is planned: {error}", file=sys.stderr)
    return out


# **A file that names the commit it was recorded at**, whole or with changes
# beside it: evidence the planner judges (`stale`). One that names none, an
# artifact or an older recipe's text, is held to what made it by its own
# test where one is (`evidence_is_current.rs`), and planned as before.
NAMES_A_COMMIT = re.compile(r"^commit: [0-9a-f]{7,40}\b", re.M)


def shown(head: str, path: str, root: pathlib.Path = ROOT) -> str | None:
    """`path` as `head` holds it, or none where it holds no such file."""
    out = subprocess.run(["git", "show", f"{head}:{path}"], cwd=root, capture_output=True)
    if out.returncode != 0:
        return None
    return out.stdout.decode("utf-8", errors="replace")


def stale(
    names: list[str],
    head: str,
    near: int = NEAR,
    body: dict[str, str] | None = None,
    show: Callable[[str, str], str | None] | None = None,
    **judge: Callable,
) -> list[str]:
    """**The recipes of `names` whose committed evidence is stale at
    `head`** (track evidence-integrity, finding 2): each writes evidence,
    every file of it, as `head` holds it, names the commit it was recorded
    at or is missing, and it is not `current` (`judge` is `current`'s own
    `read`, `descends_from` and `reach`).

    A push plans what its range reaches, less what is current, so evidence
    gone stale before the range was planned by no push; and the nightly,
    which runs every recipe, keeps no evidence. So
    `docs/evidence/E10/privacy-by-resolution.txt` said "4 of 4 mutants
    killed" from 2026-10-07, when e0752896 retired the fourth from its
    script, to this track; and 53 more recipes' evidence was stale at
    master 22a0508e. A train's push and master's plan these too
    (`--stale`), so the run whose evidence a merge takes records them
    again."""
    body = bodies() if body is None else body
    show = show or shown
    judged = []
    for name in names:
        files = evidence_files(body.get(name, ""))
        texts = [show(head, f) for f in files]
        if files and all(t is None or NAMES_A_COMMIT.search(t) for t in texts):
            judged.append(name)
    kept = current(judged, head, near, body, **judge)
    return [n for n in judged if n not in kept]


def joined(a: tuple[bool, bool], b: tuple[bool, bool]) -> tuple[bool, bool]:
    """What a shard sets up for two recipes: what either needs."""
    return (a[0] or b[0], a[1] or b[1])


def deal(
    names: list[str],
    costs: dict[str, int],
    shards: int,
    need: dict[str, tuple[bool, bool]] | None = None,
) -> list[tuple[list[str], tuple[bool, bool], int]]:
    """Deal `names` into `shards`, the costliest first, each to the shard
    where it ends soonest: its cost, and what it adds to that shard's setup
    (`SETUP`); ties to the lower index, so a deal is the same every time.
    Each shard dealt a recipe, as its recipes, what it sets up, and when it
    ends."""
    need = need or {}
    plain = (False, False)
    kinds = [plain] * shards
    ends = [SETUP[plain]] * shards
    dealt: list[list[str]] = [[] for _ in range(shards)]
    for name in sorted(names, key=lambda n: (-costs.get(n, 1), n)):
        wants = need.get(name, plain)

        def after(k: int) -> int:
            return ends[k] + costs.get(name, 1) + SETUP[joined(kinds[k], wants)] - SETUP[kinds[k]]

        i = min(range(shards), key=lambda k: (after(k), k))
        ends[i] = after(i)
        kinds[i] = joined(kinds[i], wants)
        dealt[i].append(name)
    return [(sorted(r), kinds[i], ends[i]) for i, r in enumerate(dealt) if r]


def needs(body: str) -> tuple[bool, bool]:
    """What a recipe's shard must set up (ADR-0249): browsers, for one that
    runs Playwright, itself or by the benchmark's `bench.mjs`, whose
    controls grade each stack in a browser (track evidence-integrity-2: a
    control that fails now fails its recipe); the build, for one that runs
    it, or reads what it makes: the servers, the store's pages, `pw`
    itself."""
    browsers = "playwright" in body or "bench.mjs" in body
    build = browsers or bool(re.search(r"run\.sh|feed\.sh|keyed-store\.sh|target/debug/", body))
    return browsers, build


def plan(
    names: list[str],
    costs: dict[str, int],
    shards: int,
    need: dict[str, tuple[bool, bool]] | None = None,
    database: frozenset[str] | set[str] = frozenset(),
) -> list[dict]:
    """Deal `names` into at most `shards` (`deal`), so that the run ends
    soonest.

    The recipes of `database` run in shards of their own, each beside a
    PostgreSQL of its own (ADR-0278), and the others never there: a
    database's shard names it to each recipe it runs. They take as many
    shards as make the run end soonest; among ties, the least excess over
    the ceiling in a multi-recipe shard, then the fewest database shards.
    The others are dealt into the rest, at least one where there are any.
    Theirs are numbered after the others'."""
    need = need or {}
    # Nothing to run, no shard: a push that touches no recipe plans none
    # (until 2026-10-09 this took the longest of no shards, and failed).
    if not names:
        return []
    on_database = [n for n in names if n in database]
    rest = [n for n in names if n not in database]
    # At most every shard but one for the rest, where there is any; one,
    # at least, though the run were given one shard in all.
    most = min(len(on_database), max(1, shards - (1 if rest else 0)))
    best = None
    for beside in range(1, most + 1) if on_database else [0]:
        left = max(1, min(len(rest), shards - beside)) if rest else 0
        others = deal(rest, costs, left, need)
        theirs = deal(on_database, costs, beside, need)
        ends = max(end for _, _, end in others + theirs)
        # An allowed long singleton can fix the last finish while another
        # shard can still be split below the ceiling. Break that tie before
        # preferring fewer database shards (ADR-0290, 2026-10-16).
        overflow = max(
            [0] + [end - CEILING for recipes, _, end in others + theirs if len(recipes) > 1]
        )
        score = (ends, overflow)
        if best is None or score < best[0]:
            best = (score, others, theirs)
    out = []
    if best is not None:
        for dealt, on in ((best[1], False), (best[2], True)):
            for recipes, kind, _ in dealt:
                out.append(
                    {"index": len(out), "recipes": recipes, "browsers": kind[0], "build": kind[1], "database": on}
                )
    return out


def ends(dealt: list[dict], costs: dict[str, int]) -> int:
    """When the last of `dealt`'s shards ends: what it sets up, and its
    recipes' costs."""
    return max(
        (SETUP[(s["browsers"], s["build"])] + sum(costs.get(r, 1) for r in s["recipes"]) for s in dealt),
        default=0,
    )


def planned(
    names: list[str],
    costs: dict[str, int],
    shards: int,
    need: dict[str, tuple[bool, bool]] | None = None,
    database: frozenset[str] | set[str] = frozenset(),
) -> list[dict]:
    """`plan` in `shards`, or in the fewest more whose every shard ends
    within `CEILING`, up to `MOST`: a shard the run's limit stops throws away
    every recipe it ran. A shard of one recipe that is longer by itself is
    let be: no number of shards ends it sooner."""
    def within(dealt: list[dict]) -> bool:
        return all(ends([s], costs) <= CEILING or len(s["recipes"]) == 1 for s in dealt)

    dealt = plan(names, costs, shards, need, database)
    for more in range(shards + 1, max(shards, MOST) + 1):
        if within(dealt):
            break
        dealt = plan(names, costs, more, need, database)
    return dealt


def measured(path: pathlib.Path | None = None) -> dict[str, float]:
    """Each recipe's seconds, as the last run that ran it to its end took
    them (`SECONDS`); none where the file is missing or unreadable. A key
    that begins `_` is a note, not a recipe: `_lower_bounds` names the
    recipes whose seconds are only what a run that stopped them, or that
    never ran them whole, measured (the integrator's ruling 2, train-10);
    the plan deals them by those seconds all the same."""
    try:
        return {
            k: float(v)
            for k, v in json.loads((path or SECONDS).read_text()).items()
            if not k.startswith("_")
        }
    except (OSError, ValueError, AttributeError):
        return {}


def past_the_limit(
    names: list[str], seconds: dict[str, float], need: dict[str, tuple[bool, bool]]
) -> list[tuple[str, int]]:
    """Each recipe of `names` whose recorded `seconds`, with the setup of a
    shard that runs it (`SETUP`, by what it `need`s), end past `LIMIT`: no
    shard can run it whole, and a run would find it out only when the limit
    stops its shard, hours in."""
    return [
        (n, round(seconds[n]))
        for n in names
        if n in seconds and seconds[n] + SETUP[need.get(n, (False, False))] > LIMIT
    ]


def kind(body: str) -> str:
    """What a recipe's work runs, which decides what a unit of it costs:
    `browser` where it, or a mutation script it runs, drives Playwright;
    `host` where they run the development server's tests; `core`
    otherwise."""
    texts = [body]
    for script in sorted(set(re.findall(r"scripts/([a-z0-9_]+_mutations\.py)", body))):
        try:
            texts.append((ROOT / "scripts" / script).read_text())
        except OSError:
            pass
    text = "\n".join(texts)
    if "playwright" in text:
        return "browser"
    if "pw-dev-server" in text:
        return "host"
    return "core"


def estimated(names: list[str], body: dict[str, str], seconds: dict[str, float]) -> dict[str, int]:
    """Each recipe's cost, in seconds: as measured where it was; otherwise
    itself and the mutants it plants (`cost`), each at the upper quartile of
    what a unit took the measured recipes of its kind (`kind`), or all of
    them where none of its kind is measured, or at `RATE` where none is.

    The upper quartile, not the median: what a unit costs spans two orders
    of magnitude, a type check's mutant against a browser's, and a recipe
    planned short runs past its shard's bound and is run again whole, where
    one planned long only ends its shard early. `e14-refusal`'s first run,
    planned at the median, 24 s a mutant, took about 420 s a mutant, and its
    shard ran out of time with 27 of its 30 mutants killed."""
    rates: dict[str, list[float]] = {}
    for n in seconds:
        if n in body:
            rates.setdefault(kind(body[n]), []).append(seconds[n] / cost(body[n]))
    every = [r for rs in rates.values() for r in rs]

    def rate(k: str) -> float:
        rs = sorted(rates.get(k) or every)
        return rs[(3 * len(rs)) // 4] if rs else RATE

    return {
        n: round(seconds[n]) if n in seconds else round(rate(kind(body.get(n, ""))) * cost(body.get(n, "")))
        for n in names
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--shards", type=int, default=16)
    parser.add_argument("--all", action="store_true")
    parser.add_argument("--changed", nargs=2, metavar=("BASE", "HEAD"))
    parser.add_argument("--near", type=int, default=NEAR)
    # With `--changed`: less each recipe whose committed evidence is current
    # at HEAD (`current`), as a push plans.
    parser.add_argument("--current", action="store_true")
    # With `--changed`: and each recipe whose committed evidence is stale at
    # HEAD (`stale`), as a train's push and master's plan.
    parser.add_argument("--stale", action="store_true")
    parser.add_argument("recipes", nargs="*")
    args = parser.parse_args()
    known = recipes()
    evidence = [r for r in known if EVIDENCE_RECIPE.match(r) and r not in LOCAL_ONLY]
    if args.recipes:
        unknown = [r for r in args.recipes if r not in known]
        if unknown:
            print(f"ci_plan: no recipe {unknown}", file=sys.stderr)
            return 1
        names = args.recipes
    elif args.changed:
        names = changed_recipes(args.changed[0], args.changed[1], args.near, evidence)
        if args.current:
            kept = current(names, args.changed[1], args.near)
            for name, sha in sorted(kept.items()):
                print(f"ci_plan: {name} is current, recorded at {sha[:8]}", file=sys.stderr)
            names = [n for n in names if n not in kept]
        if args.stale:
            old = [n for n in stale(evidence, args.changed[1], args.near) if n not in names]
            for name in old:
                print(f"ci_plan: {name}'s evidence is stale, so it is planned", file=sys.stderr)
            names = sorted(set(names) | set(old))
    elif args.all:
        names = evidence
    else:
        print("ci_plan: say --all, --changed BASE HEAD, or the recipes", file=sys.stderr)
        return 1
    body = bodies()
    seconds = measured()
    need = {n: needs(body.get(n, "")) for n in names}
    too_long = past_the_limit(names, seconds, need)
    for name, recorded in too_long:
        print(
            f"ci_plan: {name} is recorded at {recorded} s, past a job's limit of {LIMIT} s "
            "with its shard's setup: no shard runs it whole; split its mutants across recipes "
            "(train-10's e14-refusal) before it is planned",
            file=sys.stderr,
        )
    if too_long:
        return 1
    costs = estimated(names, body, seconds)
    print("shards=" + json.dumps(planned(names, costs, args.shards, need, NEEDS_DATABASE)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
