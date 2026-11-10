"""**A recipe's browser suite runs on what the recipe built, or not at all**
(track evidence-integrity, finding 5, 2026-10-15).

Track reviews found a browser suite run on a half-built `dist`: `run.sh`
had stopped at its step 3c, before it copied the runtime, and
`playwright.config.mjs` asked for the runtime before it asked anything else
of `dist`, so a build that stopped there was served. And two recipes,
`e14-resume-gate` and `e14-signals`, ran their suites with no build at all,
on whatever `dist` and server the worktree or the shard held.

Every recipe in the justfile and its imports that runs a browser suite
(`playwright test`) is run here as written, by `just` under the justfile's
own settings, in a tree of its own where each program it calls is a stand-in
that logs its call: the build scripts (`spikes/own-renderer/*.sh`), `cargo`,
`pnpm`, `python3`, `node`, `rustc`, `psql`, `git`. Each recipe:
- builds `dist` (`run.sh`) and the development server (`cargo build -p
  pw-dev-server`) before its first suite, its control run says;
- and stops, failing, at each build before its first suite that fails: no
  suite runs after it.
"""

from __future__ import annotations

import concurrent.futures
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

import ci_plan  # noqa: E402

# Each program a recipe calls, as a stand-in: it logs its name and its
# arguments, says what a version asks of it, and what the recipes' report
# filters look for (a filter that matches nothing fails under `pipefail`),
# and fails where told (the `STUB_FAIL_AT`th call).
STUB = r"""#!/usr/bin/env bash
name="$(basename "$0")"
n=$(( $(wc -l < "$STUB_LOG") + 1 ))
printf '%s\t%s\t%s\n' "$name" "$PWD" "$*" >> "$STUB_LOG"
case "$name $*" in
  "rustc --version"*) echo "rustc 0.0.0 (stand-in)" ;;
  "node --version"*) echo "v0.0.0" ;;
  "pnpm exec playwright --version"*) echo "Version 0.0.0" ;;
  "git rev-parse"*) echo "0000000000000000000000000000000000000000" ;;
  "psql"*) echo "18.0" ;;
  *)
    echo "test stand_in ... ok"
    echo "test_stand_in ... ok"
    echo "Ran 1 test"
    echo "OK"
    echo "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
    echo "  ✓  1 [chromium] › stand-in"
    echo "  1 passed (0.1s)"
    echo "baseline: 1 passed, 0 failed"
    echo "stand-in: KILLED (1 of 1 tests fail)"
    echo "1 of 1 mutants killed"
    echo "memory bound: no process was stopped"
    echo "page  stand-in domain.display"
    echo "0 ERRORS 0 WARNINGS"
    echo "ok 1 - stand-in"
    ;;
esac
if [ "$n" = "${STUB_FAIL_AT:-0}" ]; then
  echo "stand-in: $name $* fails" >&2
  # What a failed suite says, where the caller asks for it.
  if [ -n "${STUB_FAIL_SAYS:-}" ]; then printf '%s\n' "$STUB_FAIL_SAYS"; fi
  exit 9
fi
exit 0
"""
PROGRAMS = ("cargo", "pnpm", "python3", "node", "rustc", "psql", "git", "npx")
BUILD_SCRIPTS = (
    "run.sh",
    "feed.sh",
    "keyed-store.sh",
    "kiokun-site.sh",
    "baseline-store.sh",
    "guestbook.sh",
    "todomvc.sh",
    "rows.sh",
    "row-events.sh",
)
SUITE = re.compile(r"playwright test")

# A call: the program's name, where it ran from, and its arguments.
Call = tuple[str, str, str]


def serves_dist(body: str) -> bool:
    """Whether a recipe runs the browser suite that serves `dist`:
    Playwright, run by `scripts/run_suite.sh` from its own directory
    (`spikes/own-renderer`, unless `-C` names another: the benchmark
    harness's serves its own builds), or from that directory itself."""
    if not SUITE.search(body):
        return False
    if "cd spikes/own-renderer" in body:
        return True
    for options in re.findall(r"run_suite\.sh((?:\s+(?:-C|--runs)\s+\S+)*)\s+--\s", body):
        where = re.search(r"-C\s+(\S+)", options)
        if where is None or where.group(1).rstrip("/") == "spikes/own-renderer":
            return True
    return False


def suites(source: pathlib.Path = ROOT) -> list[str]:
    """Every recipe that runs the browser suite that serves `dist`, in every
    part of the justfile."""
    return sorted(n for n, b in ci_plan.bodies(source).items() if serves_dist(b))


def is_build(call: Call) -> bool:
    name, _, args = call
    return name in BUILD_SCRIPTS or (
        name == "cargo" and (args.startswith("build") or " -- build " in f" {args} ")
    )


def is_suite(call: Call) -> bool:
    """The browser suite that serves `dist`: Playwright, run from
    `spikes/own-renderer` (the benchmark harness's serves its own builds)."""
    name, cwd, args = call
    return (
        (name == "pnpm" and args.startswith("exec playwright test"))
        or (name == "npx" and args.startswith("playwright test"))
    ) and cwd.endswith("spikes/own-renderer")


def run(recipe: str, fail_at: int = 0, source: pathlib.Path = ROOT) -> tuple[int, list[Call], str]:
    """`recipe`, from `source`'s justfile and its imports, run by `just` in a
    tree of its own, its calls stood in for: (its exit status, its calls in
    order, what it said)."""
    status, calls, said, _ = run_full(recipe, fail_at, source)
    return status, calls, said


# The repository's directories, without their files: a recipe that runs
# from one (`cd benchmarks/harness`) finds it.
DIRECTORIES = sorted(
    {
        str(pathlib.Path(p).parent)
        for p in subprocess.run(
            ["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.splitlines()
    }
    - {"."}
)


def run_full(
    recipe: str, fail_at: int = 0, source: pathlib.Path = ROOT, fail_says: str = ""
) -> tuple[int, list[Call], str, dict[str, str]]:
    """`run`, and each evidence file the recipe left, by its path, with its
    text. The `fail_at`th call says `fail_says` as it fails."""
    with tempfile.TemporaryDirectory(prefix="pw-builds-") as d:
        root = pathlib.Path(d)
        for directory in DIRECTORIES:
            (root / directory).mkdir(parents=True, exist_ok=True)
        shutil.copy(source / "justfile", root / "justfile")
        if (source / "just").is_dir():
            shutil.copytree(source / "just", root / "just", dirs_exist_ok=True)
        # The programs, the build scripts, and what a build leaves that a
        # recipe runs or reads by its path.
        for where, names in (
            (root / ".toolchain/prefix/bin", PROGRAMS),
            (root / "spikes/own-renderer", BUILD_SCRIPTS),
            # TodoMVC's own Cypress suite, which `e14-todomvc` runs after its
            # browser suite (track `todomvc`), and its probes (track
            # `device-store`).
            (root / "spikes/todomvc-cypress", ("run.sh",)),
            (root / "spikes/todomvc-cypress-probes", ("run.sh",)),
            (root / "target/debug", ("pw", "pw-dev-server", "kiokun-server")),
            (root / "target/wasm32-unknown-unknown/browser", ("pw_resume_wasm.wasm",)),
        ):
            where.mkdir(parents=True, exist_ok=True)
            for name in names:
                (where / name).write_text(STUB)
                (where / name).chmod(0o755)
        # The helper every suite runs by (track evidence-integrity-2), as the
        # tree under test holds it.
        (root / "scripts").mkdir(exist_ok=True)
        helper = source / "scripts/run_suite.sh"
        shutil.copy(helper if helper.exists() else ROOT / "scripts/run_suite.sh", root / "scripts/run_suite.sh")
        # Where the recipes write their evidence, as the repository holds it,
        # and what one reads that no build makes: the kiokun shards' manifests,
        # and a benchmark run's result for each task (`e14-harness` lists them).
        for d in (ROOT / "docs/evidence").iterdir():
            if d.is_dir():
                (root / "docs/evidence" / d.name).mkdir(parents=True, exist_ok=True)
        for manifest in ROOT.glob("examples/kiokun/data/*/MANIFEST.txt"):
            copy = root / manifest.relative_to(ROOT)
            copy.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(manifest, copy)
        (root / "benchmarks/results").mkdir(parents=True, exist_ok=True)
        for task in (ROOT / "benchmarks/tasks").glob("T*"):
            (root / "benchmarks/results" / f"{task.name[:3]}-stand-in.json").write_text("{}\n")
        # And the harness's Playwright, whose version `e14-contract` names.
        package = root / "benchmarks/harness/node_modules/@playwright/test/package.json"
        package.parent.mkdir(parents=True, exist_ok=True)
        package.write_text('{"version": "0.0.0"}\n')
        log = root / "calls.tsv"
        log.write_text("")
        env = {
            k: v
            for k, v in os.environ.items()
            if k not in ("BASH_ENV", "ENV", "SHELLOPTS", "BASHOPTS", "JUST_SHELL", "KIOKUN_DATA")
        }
        env.update(
            STUB_LOG=str(log),
            STUB_FAIL_AT=str(fail_at),
            STUB_FAIL_SAYS=fail_says,
            NO_COLOR="1",
            # The recipes on a database skip without one.
            PW_STORE_DATABASE_URL="postgresql://stand-in/store",
            PW_FEED_DATABASE_URL="postgresql://stand-in/feed",
        )
        just = shutil.which("just")
        assert just, "just is required; this gate must not skip"
        out = subprocess.run(
            [just, "--justfile", str(root / "justfile"), recipe],
            cwd=root,
            env=env,
            capture_output=True,
            text=True,
            timeout=60,
        )
        calls = []
        for line in log.read_text().splitlines():
            name, cwd, args = (line.split("\t", 2) + ["", ""])[:3]
            calls.append((name, cwd, args))
        evidence = {
            str(p.relative_to(root)): p.read_text(errors="replace")
            for p in (root / "docs/evidence").rglob("*")
            if p.is_file()
        }
        return out.returncode, calls, out.stdout + out.stderr, evidence


def shown(call: Call) -> str:
    return " ".join(part for part in (call[0], call[2]) if part)


def problems(recipe: str, source: pathlib.Path = ROOT, failed: list | None = None) -> list[str]:
    """What is wrong with `recipe`'s builds before its first suite; each build
    failed in turn is added to `failed`."""
    status, calls, said = run(recipe, source=source)
    at = next((i for i, c in enumerate(calls) if is_suite(c)), None)
    if status != 0 or at is None:
        return [f"{recipe}: its control run reached no suite (exit {status}): {said.strip()[-300:]}"]
    before = calls[:at]
    out = []
    if not any(name == "run.sh" for name, _, _ in before):
        out.append(f"{recipe}: runs a browser suite with no `run.sh` before it, on whatever `dist` was built before")
    if not any(name == "cargo" and args.startswith("build") and "pw-dev-server" in args for name, _, args in before):
        out.append(f"{recipe}: runs a browser suite with no `cargo build -p pw-dev-server` before it")
    for k, call in enumerate(before, 1):
        if not is_build(call):
            continue
        status, calls, _ = run(recipe, fail_at=k, source=source)
        if failed is not None:
            failed.append((recipe, shown(call)))
        ran = sum(1 for c in calls[k:] if is_suite(c))
        if status == 0:
            out.append(f"{recipe}: its build `{shown(call)}` failed, and the recipe passed")
        if ran:
            out.append(f"{recipe}: its build `{shown(call)}` failed, and {ran} suite(s) ran after it")
    return out


class BuildsBeforeSuites(unittest.TestCase):
    def test_the_stand_ins_tell_a_build_that_stops_its_recipe_from_one_that_does_not(self):
        # The control for the test below, on recipes of its own under the
        # justfile's own settings: `;` under `bash -euo pipefail` stops at a
        # failed build; a build behind `|| true` does not; a suite with no
        # build runs on what was built before.
        real = (ROOT / "justfile").read_text()
        settings = real[: real.index("# The parallel tracks' recipes")]
        recipes = (
            "stops:\n"
            "    @{ BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null 2>&1; \\\n"
            "       cargo build --quiet --locked -p pw-dev-server; \\\n"
            "       (cd spikes/own-renderer && pnpm exec playwright test x.spec.mjs) || true; \\\n"
            "     } > out.txt\n"
            "masked:\n"
            "    @{ BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null 2>&1 || true; \\\n"
            "       cargo build --quiet --locked -p pw-dev-server; \\\n"
            "       (cd spikes/own-renderer && pnpm exec playwright test x.spec.mjs) || true; \\\n"
            "     } > out.txt\n"
            "unbuilt:\n"
            "    @(cd spikes/own-renderer && pnpm exec playwright test x.spec.mjs) || true\n"
        )
        with tempfile.TemporaryDirectory(prefix="pw-builds-control-") as d:
            source = pathlib.Path(d)
            (source / "justfile").write_text(settings + recipes)
            self.assertEqual(suites(source), ["masked", "stops", "unbuilt"])
            self.assertEqual(problems("stops", source), [])
            self.assertEqual(
                problems("masked", source),
                [
                    "masked: its build `run.sh` failed, and the recipe passed",
                    "masked: its build `run.sh` failed, and 1 suite(s) ran after it",
                ],
            )
            self.assertEqual(
                [p.split(",")[0] for p in problems("unbuilt", source)],
                [
                    "unbuilt: runs a browser suite with no `run.sh` before it",
                    "unbuilt: runs a browser suite with no `cargo build -p pw-dev-server` before it",
                ],
            )

    def test_every_recipe_that_runs_a_browser_suite_builds_first_and_stops_where_a_build_fails(self):
        names = suites()
        self.assertGreater(len(names), 50)
        failed: list = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            found = [p for ps in pool.map(lambda n: problems(n, failed=failed), names) for p in ps]
        print(
            f"\nbuilds before suites: {len(names)} recipes run the browser suite that serves "
            f"`dist`; {len(failed)} builds before their suites, each failed in turn",
            file=sys.stderr,
        )
        self.assertEqual(found, [], "\n".join(found))


# Since the host map (ADR-0356) the configuration asks `buildProblem`
# (`host-metadata.mjs`) of `dist` where a selected test needs it, and refuses
# what it says; it is asked here without Playwright. The planner's refusal
# itself is `host-plan.test.mjs`'s.
class DistIsWhole(unittest.TestCase):
    """`dist` is served only as a whole build, whoever runs the suite: a
    person, a script, or a recipe."""

    RUNTIME = (ROOT / "spikes/own-renderer/public/pw-runtime.mjs").read_text()

    def load(self, dist: dict[str, str] | None) -> str:
        """What node says asking `buildProblem` of `dist` (each file's path
        and text, or no `dist` at all): nothing where it is servable."""
        with tempfile.TemporaryDirectory(prefix="pw-dist-") as d:
            root = pathlib.Path(d)
            spike = ROOT / "spikes/own-renderer"
            for dependency in ("host-metadata.mjs", "host-registry.json"):
                shutil.copy(spike / dependency, root)
            (root / "public").mkdir()
            shutil.copy(spike / "public/pw-runtime.mjs", root / "public")
            for rel, text in (dist or {}).items():
                path = root / "dist" / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            metadata = (root / "host-metadata.mjs").as_uri()
            invoke = (
                f"const {{ buildProblem }} = await import({metadata!r}); "
                "const problem = buildProblem('store'); if (problem) throw new Error(problem);"
            )
            out = subprocess.run(
                ["node", "--input-type=module", "-e", invoke],
                cwd=root,
                capture_output=True,
                text=True,
                timeout=60,
            )
            return "" if out.returncode == 0 else out.stderr

    def test_a_whole_build_is_served(self):
        # The control: what `run.sh` leaves when every step ran.
        whole = {"pw-runtime.mjs": self.RUNTIME, "sources.json": '{"files": []}', "build/graph.json": "{}"}
        self.assertEqual(self.load(whole), "")

    def test_a_build_stopped_part_way_or_never_made_is_refused(self):
        # Stopped at step 3c, before the runtime was copied (track reviews'
        # case); stopped at 3d, after it; and no build at all.
        for dist in (
            {"HelloStatic.html": "<p>", "build/graph.json": "{}"},
            {"HelloStatic.html": "<p>", "build/graph.json": "{}", "pw-runtime.mjs": self.RUNTIME},
            None,
        ):
            with self.subTest(dist=sorted(dist or {})):
                self.assertIn("dist is no whole build", self.load(dist))

    def test_a_build_of_another_runtime_or_of_changed_sources_is_refused(self):
        self.assertIn(
            "dist was built with another pw-runtime.mjs",
            self.load({"pw-runtime.mjs": "// another", "sources.json": '{"files": []}'}),
        )
        with tempfile.NamedTemporaryFile("w", suffix=".pw") as source:
            source.write("module m\n")
            source.flush()
            recorded = json.dumps({"files": [{"path": source.name, "sha256": "0" * 64}]})
            self.assertIn(
                "changed since dist was built",
                self.load({"pw-runtime.mjs": self.RUNTIME, "sources.json": recorded}),
            )


if __name__ == "__main__":
    unittest.main()
