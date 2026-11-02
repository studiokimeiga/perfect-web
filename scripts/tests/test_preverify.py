"""`preverify.py`: the recipes CI's verify would run, run here in lanes, as a
pre-check that writes nothing into the tree it is run in.

A toy tree (its own git, a justfile of toy recipes, copies of the planner, the
runner, the summary, the allocator and the tool) is run by the tool as a
subprocess, and the toy recipes record where and with what environment they
ran. The tool's pure parts (ports, environment, ordering, judging, the report)
are tested on their own, and the workflow it follows is read: a test holds
the plan's flags, the Build step and CI's cargo settings to `verify.yml`.
"""

import hashlib
import importlib.util
import io
import json
import os
import pathlib
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stderr, redirect_stdout
from types import SimpleNamespace
from unittest import mock

SCRIPTS = pathlib.Path(__file__).resolve().parent.parent
ROOT = SCRIPTS.parent
sys.path.insert(0, str(SCRIPTS))


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


pv = load("preverify")
ci_summary = load("ci_summary")

WORKFLOW = (ROOT / ".github/workflows/verify.yml").read_text()

# A run's disk bound off, for every test that is not about it: the bound's
# own tests (`RunTwice`'s disk tests) state theirs from the machine's free
# disk, or one no machine has.
NO_DISK_BOUND = ("--min-free-gib", "0")

TOY = '''\
import json, os, pathlib, subprocess, sys, time

name, delay = sys.argv[1], float(sys.argv[2])
mode = sys.argv[3] if len(sys.argv) > 3 else "ok"
KEYS = ("PORT", "PW_MESSAGES_PORT", "PW_NOTIFICATIONS_PORT", "BENCH_PORT", "CARGO_TARGET_DIR",
        "CARGO_BUILD_JOBS", "TMPDIR", "KIOKUN_DATA", "KIOKUN_APP", "PW_FEED_DATABASE_URL",
        "PW_STORE_DATABASE_URL", "CARGO_INCREMENTAL", "CARGO_TERM_COLOR", "CARGO_PROFILE_DEV_DEBUG",
        "CARGO_PROFILE_TEST_DEBUG", "PW_MUTANT_SLOTS", "PW_PREVERIFY_LANE")
record = {k: os.environ.get(k) for k in KEYS}
record.update(name=name, cwd=os.getcwd(), pid=os.getpid(), start=time.time())
for key, args in (("head", ["rev-parse", "HEAD"]), ("branch", ["branch", "--show-current"])):
    record[key] = subprocess.run(["git", *args], capture_output=True, text=True).stdout.strip()
seen = os.environ.get("TOY_SEEN")


def save():
    if seen:
        pathlib.Path(seen, f"{name}.json").write_text(json.dumps(record))


save()
meet = [m for m in os.environ.get("TOY_MEET", "").split(",") if m]
if name in meet:
    others = [m for m in meet if m != name]
    waited = time.time() + 30
    while time.time() < waited and not all(pathlib.Path(seen, f"{o}.json").exists() for o in others):
        time.sleep(0.02)
    record["met"] = all(pathlib.Path(seen, f"{o}.json").exists() for o in others)
if mode == "leave":
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(120)"], start_new_session=True)
    record["child"] = child.pid
if mode == "early":
    evidence_now = pathlib.Path("docs/evidence/E14")
    evidence_now.mkdir(parents=True, exist_ok=True)
    with open(evidence_now / f"{name}.txt", "w") as out:
        out.write("toy early\\n")
        out.write("a first mutant: KILLED (1 of 3 tests fail)\\n")
        out.flush()
        out.write("an early mutant: SURVIVED\\n")
        out.flush()
        time.sleep(delay)
if mode == "hang":
    time.sleep(600)
time.sleep(delay if mode != "early" else 0)
record["end"] = time.time()
save()
evidence = pathlib.Path("docs/evidence/E14")
evidence.mkdir(parents=True, exist_ok=True)
head = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
lines = [f"toy {name}", f"commit: {head}"]
if mode == "survive":
    lines.append("a mutant is undone: SURVIVED")
if mode != "early":
    (evidence / f"{name}.txt").write_text("\\n".join(lines) + "\\n")
print(f"{name}: done")
if mode == "fail":
    print(f"test toy::{name} ... FAILED")
    print("thread 'toy' panicked at src/toy.rs:1:1:")
    sys.exit(3)
'''

JUSTFILE = """\
set shell := ["bash", "-euo", "pipefail", "-c"]

e14-long:
    @python3 scripts/toy.py long 2.5
e14-mid:
    @python3 scripts/toy.py mid 1.5
e14-short:
    @python3 scripts/toy.py short 0.2
e14-bad:
    @python3 scripts/toy.py bad 0.1 fail
e14-survivor:
    @python3 scripts/toy.py survivor 0.1 survive
e14-feed-postgres:
    @python3 scripts/toy.py feed-postgres 0.1
e14-leave:
    @python3 scripts/toy.py leave 0.2 leave
e14-hang:
    @python3 scripts/toy.py hang 0 hang
e14-builds:
    @python3 scripts/toy.py builds 0.1 ok target/debug/pw
e14-early:
    @python3 scripts/toy.py early 3.0 early
"""

SECONDS = {
    "e14-long": 250, "e14-mid": 150, "e14-short": 50, "e14-bad": 30, "e14-survivor": 20,
    "e14-feed-postgres": 10, "e14-leave": 5, "e14-hang": 500, "e14-builds": 1, "e14-early": 40,
}


def git(root, *args, check=True):
    return subprocess.run(
        ["git", "-c", "user.name=toy", "-c", "user.email=toy@example.invalid", *args],
        cwd=root, capture_output=True, text=True, check=check,
    )


def toy_tree(where: pathlib.Path) -> pathlib.Path:
    """A toy tree, committed, with the machinery beside its toy recipes."""
    root = where / "toy"
    (root / "scripts").mkdir(parents=True)
    for name in ("preverify", "ci_plan", "ci_recipes", "ci_summary", "mutation_slots"):
        shutil.copy(SCRIPTS / f"{name}.py", root / "scripts" / f"{name}.py")
    registry = root / "spikes/own-renderer/host-registry.json"
    registry.parent.mkdir(parents=True)
    shutil.copy(ROOT / "spikes/own-renderer/host-registry.json", registry)
    (root / "scripts" / "toy.py").write_text(TOY)
    (root / "scripts" / "ci_seconds.json").write_text(json.dumps(SECONDS))
    (root / "justfile").write_text(JUSTFILE)
    (root / ".gitignore").write_text(".toolchain/\nnode_modules/\ntarget/\n__pycache__/\n")
    (root / ".toolchain" / "prefix" / "bin").mkdir(parents=True)
    git(root, "init", "-q")
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "toy")
    return root


def tree_digest(root: pathlib.Path) -> str:
    """Every file under a tree but its git, by path and bytes."""
    h = hashlib.sha256()
    for p in sorted(root.rglob("*")):
        if ".git" in p.relative_to(root).parts or "__pycache__" in p.parts or not p.is_file():
            continue
        h.update(str(p.relative_to(root)).encode())
        h.update(p.read_bytes())
    return h.hexdigest()


def free_range(lanes: int = 2) -> str:
    """A range of ports nothing listens on, wide enough for `lanes` lanes."""
    stride, _ = pv.mutation_slots.port_layout(ROOT)
    for attempt in range(60):
        # Below the machine's ephemeral ports (32768-60999): a connection any
        # process opens may hold one of those, and it would be found busy.
        first = 20000 + (os.getpid() * 7919 + attempt * 2 * stride) % 11000
        if all(not pv.busy_ports(first + k * stride, ROOT) for k in range(lanes)):
            return f"{first}-{first + lanes * stride - 1}"
    raise AssertionError("no free range of ports")


class Toy(unittest.TestCase):
    """A toy tree for the test, and a way to run the tool in it."""

    def setUp(self):
        self.temp = pathlib.Path(tempfile.mkdtemp(prefix="pv-test-"))
        self.addCleanup(shutil.rmtree, self.temp, True)
        self.toy = toy_tree(self.temp)
        self.seen = self.temp / "seen"
        self.seen.mkdir()
        self.out = self.temp / "out"
        self.addCleanup(self.stop_lanes)

    def stop_lanes(self):
        """Whatever a lane started and a test left running (a hung recipe,
        a process it left behind) is stopped."""
        for k in range(8):
            pv.stop_marked(str(self.out / f"lane-{k}"), grace=1)

    def drop_lock(self, ports):
        """The lock file of a range of ports a test ran on."""
        pathlib.Path(f"/tmp/preverify-ports-{ports}.lock").unlink(missing_ok=True)

    def reap(self, proc):
        """A tool started by a test, stopped and its pipes closed."""
        if proc.poll() is None:
            proc.kill()
        proc.wait()
        for stream in (proc.stdout, proc.stderr):
            if stream:
                stream.close()

    def env(self, **extra):
        env = {k: v for k, v in os.environ.items() if not k.startswith(("PW_MUTANT_", "TOY_"))}
        env.update(
            TOY_SEEN=str(self.seen), KIOKUN_DATA="/nonexistent/kiokun", KIOKUN_APP="/nonexistent/app",
            PW_FEED_DATABASE_URL="postgresql://toy@localhost/toy", PW_STORE_DATABASE_URL="postgresql://toy/store",
            CARGO_INCREMENTAL="1", PYTHONDONTWRITEBYTECODE="1",
        )
        env.update(extra)
        return env

    def tool(self, *args, env=None, timeout=90):
        # A run that is not about the disk bound states none, so that it does
        # not depend on the machine's free disk (a runner had 6.9 GiB, under
        # the default 8 a lane: PR #67, 2026-11-02); an explicit bound later
        # in `args` overrides this one.
        if "--recipes" in args:
            args = (*NO_DISK_BOUND, *args)
        return subprocess.run(
            [sys.executable, "scripts/preverify.py", *args], cwd=self.toy, env=env or self.env(),
            capture_output=True, text=True, timeout=timeout,
        )

    def run_recipes(self, recipes, *more, lanes=2, ports=None, env=None):
        ports = ports or free_range(lanes)
        self.addCleanup(self.drop_lock, ports)
        return self.tool(
            "--recipes", recipes, "--lanes", str(lanes), "--no-setup", "--ports", ports,
            "--out", str(self.out), "--cpus", "8", *more, env=env,
        )

    def records(self):
        return {p.stem: json.loads(p.read_text()) for p in self.seen.glob("*.json")}

    def worktrees(self):
        out = git(self.toy, "worktree", "list", "--porcelain").stdout
        return [l.split(" ", 1)[1] for l in out.splitlines() if l.startswith("worktree ")]


# ---------------------------------------------------------------------------
# The lanes' ports, jobs and environment
# ---------------------------------------------------------------------------


class Ports(unittest.TestCase):
    def test_a_lane_takes_the_host_registrys_stride_from_the_first_port(self):
        stride, _ = pv.mutation_slots.port_layout(ROOT)
        for lanes in range(1, 8):
            ports = pv.lane_ports(lanes, ROOT)
            self.assertEqual(ports, [pv.FIRST_PORT + k * stride for k in range(lanes)])

    def test_no_two_lanes_share_a_port_and_each_stays_in_the_range(self):
        stride, highest = pv.mutation_slots.port_layout(ROOT)
        ports = pv.lane_ports(7, ROOT)
        taken = set()
        for p in ports:
            block = set(range(p, p + stride))
            self.assertFalse(taken & block, "two lanes' ranges overlap")
            taken |= block
            self.assertGreaterEqual(p, pv.FIRST_PORT)
            self.assertLessEqual(p + highest, pv.LAST_PORT)

    def test_the_range_is_this_workers_assignment(self):
        self.assertEqual((pv.FIRST_PORT, pv.LAST_PORT), (12741, 14740))

    def test_more_lanes_than_the_range_holds_are_refused_saying_how_many_fit(self):
        with self.assertRaises(pv.Refusal) as caught:
            pv.lane_ports(8, ROOT)
        self.assertIn("room for 7 lanes", str(caught.exception))
        self.assertIn("12741-14740", str(caught.exception))

    def test_a_lane_is_not_zero(self):
        with self.assertRaises(pv.Refusal):
            pv.lane_ports(0, ROOT)

    def test_the_spacing_is_the_registrys_not_a_number_of_this_tools(self):
        with tempfile.TemporaryDirectory() as t:
            root = pathlib.Path(t)
            registry = json.loads((ROOT / "spikes/own-renderer/host-registry.json").read_text())
            registry["stride"] = 300
            path = root / "spikes/own-renderer/host-registry.json"
            path.parent.mkdir(parents=True)
            path.write_text(json.dumps(registry))
            self.assertEqual(pv.lane_ports(3, root, 1000, 2000), [1000, 1300, 1600])
            with self.assertRaises(pv.Refusal):
                pv.lane_ports(4, root, 1000, 2000)

    def test_lanes_that_fit_the_range_exactly_are_not_refused_and_one_port_short_is(self):
        stride, _ = pv.mutation_slots.port_layout(ROOT)
        self.assertEqual(pv.lane_ports(2, ROOT, 1000, 1000 + 2 * stride - 1), [1000, 1000 + stride])
        with self.assertRaises(pv.Refusal):
            pv.lane_ports(2, ROOT, 1000, 1000 + 2 * stride - 2)

    def test_the_room_is_counted_in_whole_strides_over_the_whole_range(self):
        stride, _ = pv.mutation_slots.port_layout(ROOT)
        with self.assertRaises(pv.Refusal) as caught:
            pv.lane_ports(4, ROOT, 1000, 1000 + 3 * stride - 1)
        self.assertIn("room for 3 lanes", str(caught.exception))
        self.assertIn(f"4 lanes need {4 * stride} ports ({stride} each", str(caught.exception))
        self.assertIn(f"holds {3 * stride}", str(caught.exception))

    def test_a_range_past_the_last_port_is_refused(self):
        with self.assertRaises(pv.Refusal):
            pv.lane_ports(2, ROOT, 65400, 65535)

    def test_a_port_something_listens_on_is_found_and_a_free_one_is_not(self):
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            s.listen()
            taken = s.getsockname()[1]
            self.assertIn(taken, pv.busy_ports(taken - 7, ROOT))
        self.assertNotIn(taken, pv.busy_ports(taken - 7, ROOT))

    def test_a_port_in_time_wait_is_free_for_a_host_to_bind(self):
        """A server that closed its connection first leaves its port in
        TIME_WAIT, as the last run's hosts did; nothing listens there."""
        with socket.socket() as server:
            server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            server.bind(("127.0.0.1", 0))
            server.listen()
            port = server.getsockname()[1]
            client = socket.create_connection(("127.0.0.1", port))
            connection, _ = server.accept()
            connection.close()
            client.close()
        self.assertNotIn(port, pv.busy_ports(port - 3, ROOT))

    def test_the_first_port_of_a_range_is_checked_too(self):
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            s.listen()
            taken = s.getsockname()[1]
            self.assertIn(taken, pv.busy_ports(taken, ROOT))

    def test_the_ports_checked_are_the_registrys_reserved_ones(self):
        _, highest = pv.mutation_slots.port_layout(ROOT)
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            s.listen()
            taken = s.getsockname()[1]
            self.assertIn(taken, pv.busy_ports(taken - highest, ROOT))
            self.assertNotIn(taken, pv.busy_ports(taken + 1, ROOT))

    def test_jobs_are_the_threads_over_the_lanes_and_at_least_one(self):
        self.assertEqual(pv.jobs_per_lane(4, 32), 8)
        self.assertEqual(pv.jobs_per_lane(3, 32), 10)
        self.assertEqual(pv.jobs_per_lane(1, 32), 32)
        self.assertEqual(pv.jobs_per_lane(4, 3), 1)


class Environment(unittest.TestCase):
    BASE = {
        "PATH": "/usr/bin", "HOME": "/home/x", "KIOKUN_DATA": "/d", "KIOKUN_APP": "/a",
        "PW_FEED_DATABASE_URL": "postgresql://f", "PW_STORE_DATABASE_URL": "postgresql://s",
        "CARGO_INCREMENTAL": "1", "CARGO_PROFILE_DEV_DEBUG": "2", "PORT": "3141", "TMPDIR": "/t",
        "PW_MUTANT_SLOTS": "4", "PW_MESSAGES_PORT": "7141", "PW_NOTIFICATIONS_PORT": "6100",
        "BENCH_PORT": "3200", "CARGO_BUILD_JOBS": "99", "CARGO_TARGET_DIR": "/elsewhere",
        "PW_MUTANT_SLOT_DIR": "/s", "PW_MUTANT_PICK": "3:0", "PW_MUTANT_SLOT": "1", "PW_HOST_TEST_PORT": "1",
        "PW_PERFORMANCE": "1", "PW_REUSE": "1", "BENCH_REUSE": "1", "BENCH_STACK": "pleris",
    }

    def env(self, **extra):
        return pv.lane_environment(
            self.BASE, 12997, pathlib.Path("/lane/tree"), pathlib.Path("/lane/tmp"), 8, "/lane", **extra
        )

    def test_a_lane_has_its_own_target_temporary_directory_and_jobs(self):
        env = self.env()
        self.assertEqual(env["CARGO_TARGET_DIR"], "/lane/tree/target")
        self.assertEqual(env["TMPDIR"], "/lane/tmp")
        self.assertEqual(env["CARGO_BUILD_JOBS"], "8")
        self.assertEqual(env["PW_PREVERIFY_LANE"], "/lane")

    def test_every_family_of_ports_is_the_lanes(self):
        env = self.env()
        for name in ("PORT", "PW_MESSAGES_PORT", "PW_NOTIFICATIONS_PORT", "BENCH_PORT"):
            self.assertEqual(env[name], "12997", name)

    def test_the_cargo_settings_are_a_runners(self):
        env = self.env()
        self.assertEqual({k: env[k] for k in pv.CI_ENV}, pv.CI_ENV)
        self.assertEqual(env["CARGO_INCREMENTAL"], "0")
        self.assertEqual(env["CARGO_PROFILE_DEV_DEBUG"], "0")

    def test_a_deployments_variables_and_slots_do_not_reach_a_lane(self):
        env = self.env()
        for name in (
            "KIOKUN_DATA", "KIOKUN_APP", "PW_FEED_DATABASE_URL", "PW_STORE_DATABASE_URL", "PW_MUTANT_SLOTS",
            "PW_MUTANT_SLOT_DIR", "PW_MUTANT_PICK", "PW_MUTANT_SLOT", "PW_HOST_TEST_PORT", "PW_PERFORMANCE",
            "PW_REUSE", "BENCH_REUSE", "BENCH_STACK",
        ):
            self.assertIn(name, self.BASE, "the test does not offer it")
            self.assertNotIn(name, env, name)

    def test_what_a_lane_does_not_set_stays_the_callers(self):
        env = self.env()
        self.assertEqual(env["PATH"], "/usr/bin")
        self.assertEqual(env["HOME"], "/home/x")

    def test_a_database_is_given_only_where_asked(self):
        self.assertNotIn("PW_FEED_DATABASE_URL", self.env())
        self.assertEqual(self.env(database="postgresql://db")["PW_FEED_DATABASE_URL"], "postgresql://db")
        self.assertNotIn("PW_STORE_DATABASE_URL", self.env(database="postgresql://db"))

    def test_the_cargo_settings_are_the_workflows(self):
        top = WORKFLOW.split("\njobs:")[0]
        block = top.split("\nenv:\n", 1)[1]
        found = {}
        for line in block.splitlines():
            m = re.match(r'^  ([A-Z_]+): "?([^"#\n]*?)"?\s*(#.*)?$', line)
            if m:
                found[m.group(1)] = m.group(2)
            elif line.strip() and not line.startswith("  "):
                break
        self.assertEqual(found, pv.CI_ENV)


# ---------------------------------------------------------------------------
# What is planned, and in what order
# ---------------------------------------------------------------------------


def workflow_run(job: str, step: str) -> list[str]:
    """The lines of the `run:` of a job's named step in verify.yml."""
    lines = WORKFLOW.splitlines()
    start = lines.index(f"  {job}:")
    at = next(i for i in range(start, len(lines)) if lines[i].strip() == f"- name: {step}")
    run = next(i for i in range(at, len(lines)) if lines[i].strip().startswith("run:"))
    indent = len(lines[run + 1]) - len(lines[run + 1].lstrip())
    out = []
    for line in lines[run + 1:]:
        if line.strip() and len(line) - len(line.lstrip()) < indent:
            break
        out.append(line.strip())
    return [l for l in out if l]


class Plan(unittest.TestCase):
    def test_a_tracks_push_is_planned_as_the_workflow_plans_it(self):
        command = pv.plan_command("abc", "def", False, True, None)
        self.assertEqual(command[1:], ["scripts/ci_plan.py", "--shards", "9", "--changed", "abc", "def", "--current"])

    def test_a_trains_push_asks_for_what_is_stale_too(self):
        command = pv.plan_command("abc", "def", True, True, None)
        self.assertEqual(command[-2:], ["--current", "--stale"])

    def test_the_current_evidence_is_kept_in_only_if_asked(self):
        self.assertNotIn("--current", pv.plan_command("abc", "def", False, False, None))

    def test_recipes_named_are_planned_by_name_whatever_the_range(self):
        command = pv.plan_command("abc", "def", True, True, ["e14-a", "e14-b"])
        self.assertEqual(command[1:], ["scripts/ci_plan.py", "--shards", "9", "e14-a", "e14-b"])

    def test_the_workflows_plan_of_a_tracks_push_is_what_the_tool_asks(self):
        asked = re.search(
            r'python3 scripts/ci_plan\.py --shards (\d+) --changed "\$base" "\$GITHUB_SHA" (--current) \$stale >> "\$GITHUB_OUTPUT"',
            WORKFLOW)
        self.assertIsNotNone(asked, "verify.yml no longer plans a track's push as this tool does")
        self.assertEqual(int(asked.group(1)), pv.PLAN_SHARDS)
        self.assertIn(asked.group(2), pv.plan_command("b", "h", False, True, None))
        self.assertIn('base=$(git merge-base origin/master "$GITHUB_SHA")', WORKFLOW)
        stale = re.search(r'case "\$GITHUB_REF_NAME" in (track/train-\*)\) stale="--stale" ;; esac', WORKFLOW)
        self.assertIsNotNone(stale, "verify.yml no longer asks for what is stale on a train's branch")
        self.assertEqual(stale.group(1), pv.STALE_BRANCHES + "*")
        self.assertIn("--stale", pv.plan_command("b", "h", True, True, None))

    def test_a_plan_is_the_recipes_of_its_shards_once_each(self):
        plan = 'shards=[{"index": 0, "recipes": ["a", "b"]}, {"index": 1, "recipes": ["c", "a"]}]'
        self.assertEqual(pv.parse_plan("noise\n" + plan + "\n"), ["a", "b", "c"])
        self.assertEqual(pv.parse_plan("shards=[]\n"), [])

    def test_output_with_no_plan_is_refused(self):
        with self.assertRaises(pv.Refusal):
            pv.parse_plan("hello\n")

    def test_the_queue_is_the_costliest_first_ties_by_name(self):
        costs = {"b": 10, "a": 10, "c": 99, "d": 1}
        self.assertEqual(pv.order(["d", "b", "c", "a"], costs), ["c", "a", "b", "d"])

    def test_a_recipe_unmeasured_is_taken_as_one(self):
        self.assertEqual(pv.order(["x", "y"], {"y": 5}), ["y", "x"])

    def test_a_recipe_without_a_cost_is_taken_as_one_not_as_nothing(self):
        self.assertEqual(pv.order(["y", "x"], {"y": 1}), ["x", "y"])

    def test_the_run_directory_is_named_by_the_head_unless_told(self):
        self.assertEqual(pv.run_directory("a" * 40, None), pathlib.Path("/tmp/preverify-aaaaaaaaaaaa"))
        self.assertEqual(pv.run_directory("a" * 40, "/x/y"), pathlib.Path("/x/y"))

    def test_a_directory_named_relatively_is_the_callers_whatever_the_lanes_run_from(self):
        self.assertEqual(pv.run_directory("a" * 40, "rel/dir"), pathlib.Path.cwd() / "rel" / "dir")

    def test_a_job_carries_what_the_plan_knows_of_its_recipe(self):
        jobs = pv.jobs_of(["e14-reviews", "e14-render-merge", "e14-kiokun-cache"], ROOT)
        by = {j.recipe: j for j in jobs}
        self.assertEqual([j.recipe for j in jobs][0], max(by, key=lambda n: (by[n].cost, n)))
        self.assertTrue(by["e14-reviews"].browsers and by["e14-reviews"].build and by["e14-reviews"].database)
        self.assertFalse(by["e14-kiokun-cache"].database or by["e14-kiokun-cache"].build)
        self.assertTrue(by["e14-render-merge"].database)
        seconds = json.loads((ROOT / "scripts/ci_seconds.json").read_text())
        self.assertEqual(by["e14-reviews"].recorded, seconds["e14-reviews"])
        self.assertEqual(by["e14-reviews"].cost, round(seconds["e14-reviews"]))
        self.assertGreater(by["e14-kiokun-cache"].cost, 0)


class Steps(unittest.TestCase):
    def test_the_default_steps_are_a_shards(self):
        self.assertEqual(pv.BOOTSTRAP_STEPS, ["pnpm install --frozen-lockfile --offline"])
        run = workflow_run("recipes", "Build")
        self.assertTrue(run[0].startswith("for _ in $(seq 150)"), run)
        self.assertEqual(run[1:], pv.BUILD_STEPS)


class Planner(Toy):
    """The tool plans with the tree's own planner, as the workflow does."""

    def test_the_seconds_and_the_bodies_are_the_trees_own_not_this_repositorys(self):
        jobs = pv.jobs_of(["e14-builds", "e14-mid"], self.toy)
        self.assertEqual([j.recipe for j in jobs], ["e14-mid", "e14-builds"])
        by = {j.recipe: j for j in jobs}
        self.assertEqual((by["e14-mid"].recorded, by["e14-mid"].cost), (150, 150))
        self.assertTrue(by["e14-builds"].build)

    def second_commit(self):
        text = (self.toy / "justfile").read_text()
        (self.toy / "justfile").write_text(text.replace("toy.py mid 1.5", "toy.py mid 1.6"))
        git(self.toy, "commit", "-q", "-am", "mid changes")

    def test_the_recipes_a_range_changed_are_the_planners_own(self):
        first = git(self.toy, "rev-parse", "HEAD").stdout.strip()
        self.second_commit()
        asked = subprocess.run(
            [sys.executable, "scripts/ci_plan.py", "--shards", "9", "--changed", first, "HEAD", "--current"],
            cwd=self.toy, capture_output=True, text=True,
        )
        expected = pv.parse_plan(asked.stdout)
        self.assertEqual(expected, ["e14-mid"])
        done = self.tool("--plan", first)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("1 recipes", done.stdout)
        self.assertIn("e14-mid: planned 150 s, CI 150 s", done.stdout)
        self.assertNotIn("e14-long", done.stdout)

    def test_the_plan_is_said_as_ci_would_be_asked_it(self):
        first = git(self.toy, "rev-parse", "HEAD").stdout.strip()
        self.second_commit()
        head = git(self.toy, "rev-parse", "HEAD").stdout.strip()
        done = self.tool("--plan", first)
        self.assertIn(
            f"preverify: planned as verify.yml plans a track's push: "
            f"python3 scripts/ci_plan.py --shards 9 --changed {first} {head} --current", done.stdout)
        named = self.tool("--plan", "--recipes", "e14-short e14-mid")
        self.assertIn(
            "preverify: planned as verify.yml plans a track's push: python3 scripts/ci_plan.py --shards 9 e14-short e14-mid",
            named.stdout)

    def test_the_base_is_a_ref_and_the_range_starts_at_its_merge_base(self):
        git(self.toy, "branch", "base")
        self.second_commit()
        done = self.tool("--plan", "base")
        self.assertIn("e14-mid", done.stdout)
        self.assertIn(git(self.toy, "rev-parse", "base").stdout.strip()[:12], done.stdout)

    def test_a_base_that_is_no_ref_is_refused(self):
        done = self.tool("--plan", "origin/nothing")
        self.assertEqual(done.returncode, 2)
        self.assertIn("no merge base of `origin/nothing`", done.stderr)

    def test_a_range_that_changed_no_recipe_runs_nothing_and_succeeds(self):
        done = self.tool("--plan", "HEAD")
        self.assertEqual(done.returncode, 0)
        self.assertIn("nothing to run", done.stdout)

    def test_a_recipe_that_is_none_is_the_planners_refusal(self):
        done = self.tool("--plan", "--recipes", "e14-nothing")
        self.assertEqual(done.returncode, 2)
        self.assertIn("scripts/ci_plan.py refused the plan", done.stderr)
        self.assertIn("no recipe", done.stderr)

    def test_the_plan_says_the_queue_the_ports_and_what_each_recipe_sets_up(self):
        done = self.tool("--plan", "--recipes", "e14-short e14-long e14-feed-postgres e14-builds", "--lanes", "3")
        self.assertEqual(done.returncode, 0, done.stderr)
        stride, _ = pv.mutation_slots.port_layout(ROOT)
        ports = ", ".join(str(pv.FIRST_PORT + k * stride) for k in range(3))
        self.assertIn(f"3 lanes on PORT {ports}", done.stdout)
        names = re.findall(r"^  (e14-[a-z-]+):", done.stdout, re.M)
        self.assertEqual(names, ["e14-long", "e14-short", "e14-feed-postgres", "e14-builds"])
        self.assertIn("e14-feed-postgres: planned 10 s, CI 10 s (database)", done.stdout)
        self.assertIn("e14-builds: planned 1 s, CI 1 s (build)", done.stdout)

    def test_recipes_named_twice_are_planned_once(self):
        done = self.tool("--plan", "--recipes", "e14-short e14-mid", "--recipes", "e14-short")
        self.assertEqual(re.findall(r"^  (e14-[a-z-]+):", done.stdout, re.M), ["e14-mid", "e14-short"])
        self.assertIn("2 recipes at", done.stdout)

    def test_lanes_of_none_are_refused(self):
        done = self.tool("--plan", "--recipes", "e14-short", "--lanes", "0")
        self.assertEqual(done.returncode, 2)
        self.assertIn("give a whole number of lanes, 1 or more", done.stderr)

    def test_current_evidence_is_left_out_as_a_push_leaves_it_and_the_planners_note_is_passed_on(self):
        first = git(self.toy, "rev-parse", "HEAD").stdout.strip()
        (self.toy / "justfile").write_text(
            JUSTFILE + "e14-current:\n    @mkdir -p docs/evidence/E14\n"
            '    @echo "commit: $(git rev-parse HEAD)" > docs/evidence/E14/current.txt\n')
        git(self.toy, "commit", "-q", "-am", "a recipe that records its evidence")
        second = git(self.toy, "rev-parse", "HEAD").stdout.strip()
        (self.toy / "docs/evidence/E14").mkdir(parents=True)
        (self.toy / "docs/evidence/E14/current.txt").write_text(f"commit: {second}\n")
        git(self.toy, "add", "-A")
        git(self.toy, "commit", "-q", "-m", "its evidence, recorded at the commit before")
        left_out = self.tool("--plan", first)
        self.assertEqual(left_out.returncode, 0, left_out.stderr)
        self.assertIn(f"ci_plan: e14-current is current, recorded at {second[:8]}", left_out.stdout)
        self.assertIn("nothing to run", left_out.stdout)
        kept = self.tool("--plan", first, "--no-current")
        self.assertIn("e14-current: planned", kept.stdout)
        self.assertNotIn("is current", kept.stdout)

    def test_a_root_that_is_no_git_is_refused_in_gits_words(self):
        with tempfile.TemporaryDirectory() as t:
            buffer = io.StringIO()
            # git looks upward for a repository, and a temporary directory may be under one
            ceiling = {"GIT_CEILING_DIRECTORIES": str(pathlib.Path(t).parent)}
            with mock.patch.dict(os.environ, ceiling), redirect_stderr(buffer):
                code = pv.main(["--plan", "--recipes", "e14-a"], pathlib.Path(t))
        self.assertEqual(code, 2)
        self.assertIn("preverify: `git rev-parse HEAD` failed", buffer.getvalue())

    def test_a_run_interrupted_before_the_lanes_exist_is_said_not_a_traceback(self):
        buffer = io.StringIO()
        with mock.patch.object(pv, "planned", side_effect=KeyboardInterrupt), redirect_stderr(buffer):
            code = pv.main(["--recipes", "e14-short"], self.toy)
        self.assertEqual(code, 130)
        self.assertIn("preverify: interrupted", buffer.getvalue())

    def test_no_more_lanes_than_recipes(self):
        done = self.tool("--plan", "--recipes", "e14-short e14-long", "--lanes", "6")
        self.assertIn("2 lanes on PORT", done.stdout)

    def test_a_train_branch_asks_for_what_is_stale_and_another_does_not(self):
        text = (self.toy / "scripts/ci_plan.py").read_text()
        tail = 'if __name__ == "__main__":\n    sys.exit(main())'
        self.assertEqual(text.count(tail), 1)
        (self.toy / "scripts/ci_plan.py").write_text(text.replace(
            tail,
            'if __name__ == "__main__":\n'
            '    import json as _j, os as _o, pathlib as _p\n'
            '    _p.Path(_o.environ["TOY_SEEN"], "argv.json").write_text(_j.dumps(sys.argv[1:]))\n'
            '    sys.exit(main())'))
        git(self.toy, "commit", "-q", "-am", "a planner that records what it is asked")
        base = git(self.toy, "rev-parse", "HEAD").stdout.strip()

        def asked(branch):
            git(self.toy, "checkout", "-q", "-B", branch)
            self.tool("--plan", base)
            return json.loads((self.seen / "argv.json").read_text())

        on_train = asked("track/train-12")
        self.assertEqual(on_train, ["--shards", "9", "--changed", base, base, "--current", "--stale"])
        on_track = asked("track/other")
        self.assertEqual(on_track, ["--shards", "9", "--changed", base, base, "--current"])
        (self.seen / "argv.json").unlink()
        self.tool("--plan", base, "--stale")
        self.assertEqual(json.loads((self.seen / "argv.json").read_text())[-1], "--stale")
        git(self.toy, "checkout", "-q", "-B", "track/train-13")
        self.tool("--plan", base, "--no-stale", "--no-current")
        self.assertEqual(
            json.loads((self.seen / "argv.json").read_text()), ["--shards", "9", "--changed", base, base]
        )


# ---------------------------------------------------------------------------
# A run
# ---------------------------------------------------------------------------


class CleanTree(Toy):
    def test_a_changed_file_is_refused_and_said_with_why(self):
        (self.toy / "justfile").write_text(JUSTFILE + "\n# a change\n")
        done = self.run_recipes("e14-short", lanes=1)
        self.assertEqual(done.returncode, 2)
        self.assertIn("the working tree is not clean (1 paths: M justfile)", done.stderr)
        self.assertIn("commit or stash", done.stderr)
        self.assertFalse(self.out.exists(), "a refused run made its directory")
        self.assertEqual(self.records(), {})

    def test_an_untracked_file_is_a_change_too(self):
        (self.toy / "notes.txt").write_text("not committed\n")
        done = self.run_recipes("e14-short", lanes=1)
        self.assertEqual(done.returncode, 2)
        self.assertIn("?? notes.txt", done.stderr)

    def test_many_changes_are_counted_and_five_shown(self):
        for n in range(8):
            (self.toy / f"n{n}.txt").write_text("x\n")
        done = self.run_recipes("e14-short", lanes=1)
        self.assertIn("8 paths", done.stderr)
        self.assertIn("and 3 more", done.stderr)

    def test_ignored_files_are_not_changes(self):
        (self.toy / ".toolchain" / "x").write_text("x\n")
        (self.toy / "target").mkdir()
        self.assertEqual(self.run_recipes("e14-short", lanes=1).returncode, 0)

    def test_what_differs_from_head_is_listed_as_git_status_lists_it(self):
        self.assertEqual(pv.dirty_paths(self.toy), [])
        (self.toy / "scripts/toy.py").write_text("raise SystemExit(9)\n")
        (self.toy / "new.txt").write_text("x\n")
        self.assertEqual(sorted(pv.dirty_paths(self.toy)), [" M scripts/toy.py", "?? new.txt"])

    def test_a_tree_with_no_toolchain_is_told_so(self):
        shutil.rmtree(self.toy / ".toolchain")
        done = self.run_recipes("e14-short", lanes=1)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("this tree has no .toolchain (run just bootstrap)", done.stderr)

    def test_a_tree_with_one_is_not(self):
        done = self.run_recipes("e14-short", lanes=1)
        self.assertNotIn(".toolchain", done.stderr)

    def test_a_plan_alone_needs_no_clean_tree(self):
        (self.toy / "notes.txt").write_text("x\n")
        self.assertEqual(self.tool("--plan", "--recipes", "e14-short").returncode, 0)


class Lanes(Toy):
    """One run of six toy recipes in two lanes, looked at from every side."""

    @classmethod
    def setUpClass(cls):
        cls.temp = pathlib.Path(tempfile.mkdtemp(prefix="pv-lanes-"))
        cls.toy = toy_tree(cls.temp)
        cls.seen = cls.temp / "seen"
        cls.seen.mkdir()
        cls.out = cls.temp / "out"
        cls.before = tree_digest(cls.toy)
        cls.status_before = git(cls.toy, "status", "--porcelain").stdout
        env = {k: v for k, v in os.environ.items() if not k.startswith(("PW_MUTANT_", "TOY_"))}
        env.update(
            TOY_SEEN=str(cls.seen), TOY_MEET="long,mid", KIOKUN_DATA="/nonexistent/kiokun",
            KIOKUN_APP="/nonexistent/app", PW_FEED_DATABASE_URL="postgresql://toy@localhost/toy",
            PW_STORE_DATABASE_URL="postgresql://toy/store", CARGO_INCREMENTAL="1", PYTHONDONTWRITEBYTECODE="1",
        )
        cls.ports = free_range(2)
        cls.first = int(cls.ports.split("-")[0])
        cls.done = subprocess.run(
            [sys.executable, "scripts/preverify.py", *NO_DISK_BOUND, "--recipes",
             "e14-short e14-bad e14-long e14-survivor e14-mid e14-feed-postgres",
             "--lanes", "2", "--no-setup", "--ports", cls.ports, "--out", str(cls.out), "--cpus", "8"],
            cwd=cls.toy, env=env, capture_output=True, text=True, timeout=180,
        )
        cls.records_ = {p.stem: json.loads(p.read_text()) for p in cls.seen.glob("*.json")}
        cls.summary = json.loads((cls.out / "summary.json").read_text())
        cls.after = tree_digest(cls.toy)
        cls.status_after = git(cls.toy, "status", "--porcelain").stdout
        cls.worktrees_after = git(cls.toy, "worktree", "list", "--porcelain").stdout

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.temp, ignore_errors=True)

    def setUp(self):  # the class's run is shared
        pass

    def test_a_failed_recipe_makes_the_exit_one_and_the_run_still_finishes_the_rest(self):
        self.assertEqual(self.done.returncode, 1, self.done.stdout + self.done.stderr)
        self.assertEqual(set(self.records_), {"short", "bad", "long", "survivor", "mid", "feed-postgres"})
        for name, record in self.records_.items():
            self.assertIn("end", record, f"{name} did not run to its end")

    def test_every_recipe_runs_once_in_a_lane(self):
        results = {r["recipe"]: r for r in self.summary["results"]}
        self.assertEqual(len(self.summary["results"]), 6)
        self.assertEqual(set(r["lane"] for r in results.values()), {0, 1})

    def test_a_lane_has_its_own_ports_target_temporary_directory_and_jobs(self):
        stride, _ = pv.mutation_slots.port_layout(ROOT)
        by_lane = {}
        for record in self.records_.values():
            lane = by_lane.setdefault(record["PORT"], [])
            lane.append(record)
            self.assertIn(int(record["PORT"]), [self.first, self.first + stride])
            for name in ("PW_MESSAGES_PORT", "PW_NOTIFICATIONS_PORT", "BENCH_PORT"):
                self.assertEqual(record[name], record["PORT"])
            self.assertEqual(record["CARGO_BUILD_JOBS"], "4")
            tree = pathlib.Path(record["cwd"]).resolve()
            self.assertEqual(pathlib.Path(record["CARGO_TARGET_DIR"]).resolve(), tree / "target")
            self.assertEqual(pathlib.Path(record["TMPDIR"]).resolve(), tree.parent / "tmp")
            self.assertEqual(pathlib.Path(record["PW_PREVERIFY_LANE"]).resolve(), tree.parent)
            self.assertEqual(tree.name, "tree")
            self.assertEqual(tree.parent.parent, self.out.resolve())
        self.assertEqual(len(by_lane), 2)
        for port, records in by_lane.items():
            self.assertEqual(len({r["cwd"] for r in records}), 1, "one lane, one worktree")

    def test_a_lane_is_a_detached_worktree_of_head(self):
        head = git(self.toy, "rev-parse", "HEAD").stdout.strip()
        for record in self.records_.values():
            self.assertEqual(record["head"], head)
            self.assertEqual(record["branch"], "")

    def test_a_lane_has_a_runners_environment(self):
        for record in self.records_.values():
            self.assertIsNone(record["KIOKUN_DATA"])
            self.assertIsNone(record["KIOKUN_APP"])
            self.assertIsNone(record["PW_STORE_DATABASE_URL"])
            self.assertIsNone(record["PW_MUTANT_SLOTS"])
            self.assertEqual(record["CARGO_INCREMENTAL"], "0")
            self.assertEqual(record["CARGO_TERM_COLOR"], "never")
            self.assertEqual(record["CARGO_PROFILE_DEV_DEBUG"], "0")

    def test_a_database_reaches_only_the_recipe_that_needs_one(self):
        for name, record in self.records_.items():
            want = "postgresql://toy@localhost/toy" if name == "feed-postgres" else None
            self.assertEqual(record["PW_FEED_DATABASE_URL"], want, name)

    def test_the_costliest_recipes_start_first(self):
        starts = sorted(self.records_.values(), key=lambda r: r["start"])
        self.assertEqual({r["name"] for r in starts[:2]}, {"long", "mid"})
        self.assertLess(self.records_["long"]["start"], self.records_["short"]["start"])
        self.assertLess(self.records_["mid"]["start"], self.records_["bad"]["start"])

    def test_the_lanes_run_at_the_same_time(self):
        """`long` and `mid` each wait for the other to have started, so that
        each having met the other is a proof that they ran at once."""
        long, mid = self.records_["long"], self.records_["mid"]
        self.assertTrue(long["met"] and mid["met"])
        self.assertNotEqual(long["PORT"], mid["PORT"])

    def test_a_recipe_that_exits_non_zero_fails_with_its_exit_and_its_failing_lines(self):
        text = (self.out / "summary.txt").read_text()
        self.assertIn("- e14-bad: exit 3", text)
        self.assertIn("--- e14-bad: exit 3 (lane ", text)
        self.assertIn("failing test lines:", text)
        self.assertIn("| test toy::bad ... FAILED", text)
        self.assertIn("| thread 'toy' panicked at src/toy.rs:1:1:", text)

    def test_a_survivor_in_the_evidence_fails_a_recipe_that_exited_zero(self):
        results = {r["recipe"]: r for r in self.summary["results"]}
        self.assertEqual(results["e14-survivor"]["status"], 0)
        self.assertEqual(results["e14-survivor"]["verdict"], "FAILED")
        self.assertEqual(results["e14-survivor"]["reason"], "a mutant is undone: SURVIVED")
        self.assertIn("- e14-survivor: a mutant is undone: SURVIVED", (self.out / "summary.txt").read_text())

    def test_the_others_pass(self):
        results = {r["recipe"]: r for r in self.summary["results"]}
        for name in ("e14-short", "e14-long", "e14-mid", "e14-feed-postgres"):
            self.assertEqual(results[name]["verdict"], "ok", name)
            self.assertEqual(results[name]["wrote"], [f"docs/evidence/E14/{name[4:]}.txt"])

    def test_the_report_is_cis_with_each_recipes_lane_and_ci_seconds(self):
        text = self.done.stdout
        self.assertIn("**6 recipes, 2 failed**", text)
        self.assertIn("| recipe | result | seconds | lane | CI seconds | files |", text)
        self.assertRegex(text, r"\| `e14-long` \| ok \| [\d.]+ \| [01] \| 250 \| 1 \|")
        self.assertRegex(text, r"\| `e14-bad` \| FAILED \| [\d.]+ \| [01] \| 30 \| 1 \|")
        self.assertIn("CI recorded 510 s for the 6 it has measured, the longest 250 s", text)
        self.assertIn("load average (1 min):", text)
        self.assertIn('just preverify origin/master --recipes "e14-bad e14-survivor"', text)

    def test_the_wall_clock_is_the_runs_own(self):
        wall = self.summary["meta"]["wall"]
        self.assertTrue(0 < wall < 120, wall)
        self.assertEqual(self.summary["meta"]["lanes"], 2)
        self.assertEqual(self.summary["meta"]["cpus"], 8)
        self.assertEqual(self.summary["meta"]["head"], git(self.toy, "rev-parse", "HEAD").stdout.strip())

    def test_the_report_is_kept_beside_the_logs(self):
        self.assertTrue(self.done.stdout.endswith((self.out / "summary.txt").read_text()))
        self.assertTrue((self.out / "plan.json").is_file())
        plan = json.loads((self.out / "plan.json").read_text())
        self.assertEqual(plan["lanes"], 2)
        self.assertEqual([r["recipe"] for r in plan["recipes"]][:2], ["e14-long", "e14-mid"])

    def test_the_tree_it_was_run_in_is_untouched(self):
        self.assertEqual(self.after, self.before)
        self.assertEqual(self.status_after, self.status_before)
        self.assertFalse((self.toy / "docs").exists(), "evidence was written into the tree")

    def test_the_evidence_and_logs_are_kept_under_the_run_directory(self):
        for result in self.summary["results"]:
            out = pathlib.Path(result["out"])
            self.assertEqual(out.parent.parent.parent.resolve(), self.out.resolve())
            name = result["recipe"]
            self.assertTrue((out / "logs" / f"{name}.log").is_file())
            self.assertTrue((out / "results.json").is_file())
            self.assertTrue((out / "evidence" / "docs/evidence/E14" / f"{name[4:]}.txt").is_file())

    def test_the_lanes_worktrees_are_taken_away_and_the_logs_stay(self):
        self.assertEqual(self.worktrees_after.count("worktree "), 1)
        for lane in (0, 1):
            directory = self.out / f"lane-{lane}"
            self.assertFalse((directory / "tree").exists())
            self.assertFalse((directory / "tmp").exists())
            self.assertTrue((directory / "out").is_dir())

    def test_a_run_says_where_the_logs_are_and_that_it_takes_the_worktrees_away(self):
        text = self.done.stdout
        self.assertIn(f"logs under {self.out}", text)
        self.assertIn("taking the lanes' worktrees away (the logs and evidence stay)", text)
        self.assertRegex(text, r"lane [01]: e14-long started; log \S+/lane-[01]/out/e14-long/logs/e14-long\.log")

    def test_a_run_says_what_it_is_doing(self):
        text = self.done.stdout
        self.assertRegex(text, r"\[\d\d:\d\d:\d\d\] preverify [0-9a-f]{12}: 6 recipes, 2 lanes on PORT ")
        self.assertRegex(text, r"lane [01]: e14-long started")
        self.assertRegex(text, r"lane [01]: e14-long ok in [\d.]+ s")
        self.assertRegex(text, r"lane [01]: e14-bad FAILED \(exit 3\) in [\d.]+ s; log ")

    def test_the_agreement_with_cis_own_summary(self):
        """CI's `ci_summary.py`, run on the lanes' output laid out as a run's
        artifacts, fails the same recipes."""
        with tempfile.TemporaryDirectory() as t:
            artifacts = pathlib.Path(t)
            for lane in (0, 1):
                shard = artifacts / f"evidence-{lane}"
                (shard / "evidence").mkdir(parents=True)
                rows = []
                for result in self.summary["results"]:
                    if result["lane"] != lane:
                        continue
                    out = pathlib.Path(result["out"])
                    rows += json.loads((out / "results.json").read_text())
                    shutil.copytree(out / "evidence", shard / "evidence", dirs_exist_ok=True)
                (shard / "results.json").write_text(json.dumps(rows))
            done = subprocess.run(
                [sys.executable, str(SCRIPTS / "ci_summary.py"), str(artifacts), "--shards", "2"],
                capture_output=True, text=True,
            )
        self.assertEqual(done.returncode, 1)
        theirs = set(re.findall(r"^\| `([^`]+)` \| FAILED", done.stdout, re.M))
        ours = {r["recipe"] for r in self.summary["results"] if r["verdict"] != "ok"}
        self.assertEqual(theirs, ours)
        self.assertEqual(ours, {"e14-bad", "e14-survivor"})


class RunTwice(Toy):
    def test_an_earlier_run_at_the_same_head_is_kept_beside_the_new_one(self):
        ports = free_range(1)
        self.assertEqual(self.run_recipes("e14-short", lanes=1, ports=ports).returncode, 0)
        (self.out / "marker.txt").write_text("the first\n")
        self.assertEqual(self.run_recipes("e14-mid", lanes=1, ports=ports).returncode, 0)
        previous = self.out.with_name(self.out.name + ".prev")
        self.assertEqual((previous / "marker.txt").read_text(), "the first\n")
        self.assertFalse((self.out / "marker.txt").exists())
        self.assertEqual([r["recipe"] for r in json.loads((self.out / "summary.json").read_text())["results"]], ["e14-mid"])

    def test_a_relative_directory_is_made_where_the_caller_asked(self):
        ports = free_range(1)
        self.addCleanup(self.drop_lock, ports)
        done = self.tool("--recipes", "e14-short", "--lanes", "1", "--no-setup", "--ports", ports,
                         "--out", "../rel-out", "--cpus", "8")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue((self.temp / "rel-out" / "summary.json").is_file())
        self.assertEqual(pathlib.Path(self.records()["short"]["cwd"]).resolve(),
                         (self.temp / "rel-out" / "lane-0" / "tree").resolve())

    def test_only_the_run_before_is_kept(self):
        ports = free_range(1)
        for marker in ("one", "two", "three"):
            self.assertEqual(self.run_recipes("e14-short", lanes=1, ports=ports).returncode, 0)
            (self.out / "marker.txt").write_text(marker)
        previous = self.out.with_name(self.out.name + ".prev")
        self.assertEqual((previous / "marker.txt").read_text(), "two")
        self.assertEqual((self.out / "marker.txt").read_text(), "three")

    def test_a_directory_that_is_no_earlier_run_is_never_touched(self):
        self.out.mkdir()
        (self.out / "precious.txt").write_text("mine\n")
        done = self.run_recipes("e14-short", lanes=1)
        self.assertEqual(done.returncode, 2)
        self.assertIn("is no earlier run's", done.stderr)
        self.assertEqual((self.out / "precious.txt").read_text(), "mine\n")
        self.assertEqual(self.records(), {})

    def test_a_previous_that_is_no_earlier_run_is_not_deleted_either(self):
        previous = self.out.with_name(self.out.name + ".prev")
        previous.mkdir()
        (previous / "precious.txt").write_text("mine\n")
        done = self.run_recipes("e14-short", lanes=1)
        self.assertEqual(done.returncode, 2)
        self.assertEqual((previous / "precious.txt").read_text(), "mine\n")

    def test_a_run_still_going_is_not_taken_from(self):
        self.out.mkdir()
        (self.out / "plan.json").write_text("{}\n")
        (self.out / "pid").write_text(f"{os.getppid()}\n")
        done = self.run_recipes("e14-short", lanes=1)
        self.assertEqual(done.returncode, 2)
        self.assertIn(f"another preverify run (pid {os.getppid()}) is using", done.stderr)
        self.assertTrue((self.out / "plan.json").exists())

    def test_the_left_overs_of_a_run_that_died_are_cleaned_first(self):
        # A run killed outright leaves its lane registered in the tree's git.
        self.out.mkdir()
        (self.out / "plan.json").write_text("{}\n")
        gone = subprocess.Popen(["true"])
        gone.wait()
        (self.out / "pid").write_text(f"{gone.pid}\n")
        tree = self.out / "lane-0" / "tree"
        git(self.toy, "worktree", "add", "--quiet", "--detach", str(tree), "HEAD")
        self.assertEqual(len(self.worktrees()), 2)
        done = self.run_recipes("e14-short", lanes=1)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(len(self.worktrees()), 1)

    def test_a_port_in_use_is_refused_before_anything_runs(self):
        ports = free_range(1)
        first = int(ports.split("-")[0])
        with socket.socket() as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind(("127.0.0.1", first + 3))
            s.listen()
            done = self.run_recipes("e14-short", lanes=1, ports=ports)
        self.assertEqual(done.returncode, 2)
        self.assertIn(f"port(s) {first + 3} in a lane's range are in use already", done.stderr)
        self.assertFalse(self.out.exists())

    def test_too_little_disk_is_refused_before_an_earlier_run_is_moved(self):
        self.assertEqual(self.run_recipes("e14-short", lanes=1).returncode, 0)
        done = self.run_recipes("e14-short", "--min-free-gib", "999999999", lanes=1)
        self.assertEqual(done.returncode, 2)
        self.assertIn("GiB free under", done.stderr)
        self.assertTrue((self.out / "plan.json").is_file(), "the earlier run was moved aside by a refused one")
        self.assertFalse(self.out.with_name(self.out.name + ".prev").exists())

    def test_the_disk_needed_is_for_each_lane(self):
        free = shutil.disk_usage(self.temp).free / 2**30
        two = self.run_recipes("e14-short e14-mid", "--min-free-gib", str(free * 0.75), lanes=2)
        self.assertEqual(two.returncode, 2, two.stdout)
        self.assertIn("and 2 lanes need", two.stderr)
        one = self.run_recipes("e14-short", "--min-free-gib", str(free * 0.75), lanes=1)
        self.assertEqual(one.returncode, 0, one.stderr)

    def test_the_disk_is_read_where_the_directory_would_be_made(self):
        self.assertGreater(pv.free_gib(self.temp / "not" / "made" / "yet"), 0)
        self.assertAlmostEqual(pv.free_gib(self.temp), pv.free_gib(self.temp / "nothing"), delta=1.0)

    def test_lanes_the_range_cannot_hold_are_refused(self):
        stride, _ = pv.mutation_slots.port_layout(ROOT)
        done = self.run_recipes("e14-short e14-mid e14-long", lanes=3, ports=f"40000-{40000 + 2 * stride - 1}")
        self.assertEqual(done.returncode, 2)
        self.assertIn("room for 2 lanes", done.stderr)


class Locks(Toy):
    def test_the_lock_names_its_owner_and_is_let_go_with_the_run(self):
        first = 20000 + os.getpid() % 30000  # this process's own, so that tests run at once do not meet
        last = first + 255
        path = pathlib.Path(f"/tmp/preverify-ports-{first}-{last}.lock")
        self.addCleanup(path.unlink, True)
        with pv.port_lock(first, last):
            self.assertEqual(path.read_text().strip(), str(os.getpid()))
            with self.assertRaises(pv.Refusal) as caught:
                with pv.port_lock(first, last):
                    pass
            self.assertIn(f"another preverify run (pid {os.getpid()}) holds the ports {first}-{last}", str(caught.exception))
            self.assertIn("--ports", str(caught.exception))
        with pv.port_lock(first, last):
            pass

    def test_two_runs_do_not_share_ports(self):
        ports = free_range(1)
        first, last = map(int, ports.split("-"))
        self.addCleanup(pathlib.Path(f"/tmp/preverify-ports-{first}-{last}.lock").unlink, True)
        with pv.port_lock(first, last):
            done = self.run_recipes("e14-short", lanes=1, ports=ports)
        self.assertEqual(done.returncode, 2)
        self.assertIn(f"another preverify run (pid {os.getpid()}) holds the ports {ports}", done.stderr)
        self.assertEqual(self.records(), {})
        self.assertFalse(self.out.exists())
        again = self.run_recipes("e14-short", lanes=1, ports=ports)
        self.assertEqual(again.returncode, 0, again.stderr)

    def test_a_run_on_other_ports_is_not_held_up(self):
        ports = free_range(1)
        first, last = map(int, ports.split("-"))
        other = f"{first + 1024}-{last + 1024}"
        for a, b in ((first, last), (first + 1024, last + 1024)):
            self.addCleanup(pathlib.Path(f"/tmp/preverify-ports-{a}-{b}.lock").unlink, True)
        with pv.port_lock(first, last):
            done = self.run_recipes("e14-short", lanes=1, ports=other)
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_a_plan_needs_no_ports(self):
        ports = free_range(1)
        first, last = map(int, ports.split("-"))
        self.addCleanup(pathlib.Path(f"/tmp/preverify-ports-{first}-{last}.lock").unlink, True)
        with pv.port_lock(first, last):
            done = self.tool("--plan", "--recipes", "e14-short", "--ports", ports)
        self.assertEqual(done.returncode, 0, done.stderr)


class Outcomes(Toy):
    def test_the_lanes_worktrees_are_kept_if_asked(self):
        done = self.run_recipes("e14-short", "--keep-lanes", lanes=1)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue((self.out / "lane-0" / "tree" / "justfile").is_file())
        self.assertEqual(len(self.worktrees()), 2)
        self.assertNotIn("taking the lanes' worktrees away", done.stdout)

    def test_a_process_a_recipe_leaves_running_is_stopped_and_said(self):
        done = self.run_recipes("e14-leave", lanes=1)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        child = self.records()["leave"]["child"]
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and pv.alive(child):
            time.sleep(0.1)
        self.assertFalse(pv.alive(child), "the left process still runs")
        self.assertIn("Left running by a recipe, and stopped", done.stdout)
        self.assertIn("- e14-leave: left 1 process(es) running, stopped after it", done.stdout)

    def test_a_recipe_past_its_time_is_stopped_and_fails(self):
        started = time.monotonic()
        done = self.run_recipes("e14-hang e14-short", "--timeout", "4", lanes=2)
        self.assertLess(time.monotonic() - started, 60)
        self.assertEqual(done.returncode, 1)
        self.assertIn("- e14-hang: timed out", done.stdout)
        hung = self.records()["hang"]["pid"]
        self.assertFalse(pv.alive(hung), "the hung recipe still runs")
        results = {r["recipe"]: r for r in json.loads((self.out / "summary.json").read_text())["results"]}
        self.assertEqual(results["e14-short"]["verdict"], "ok")

    def test_a_database_recipe_with_no_database_is_not_run_and_fails_the_run(self):
        env = self.env()
        del env["PW_FEED_DATABASE_URL"]
        done = self.run_recipes("e14-feed-postgres e14-short", lanes=2, env=env)
        self.assertEqual(done.returncode, 1)
        self.assertIn("- e14-feed-postgres: not run: PW_FEED_DATABASE_URL is not set", done.stdout)
        self.assertNotIn("feed-postgres", self.records())
        self.assertIn("short", self.records())
        self.assertIn("| `e14-feed-postgres` | NOT RUN |", done.stdout)

    def test_a_clean_run_exits_zero(self):
        done = self.run_recipes("e14-short e14-mid", lanes=2)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertIn("**2 recipes, 0 failed**", done.stdout)

    def test_a_lane_runs_one_recipe_at_a_time_and_takes_the_next_when_free(self):
        done = self.run_recipes("e14-short e14-mid e14-long e14-bad", lanes=2, env=self.env(TOY_MEET="long,mid"))
        records = self.records()
        self.assertEqual(len(records), 4, done.stdout)
        self.assertTrue(records["long"]["met"] and records["mid"]["met"])
        for port in {r["PORT"] for r in records.values()}:
            mine = sorted((r for r in records.values() if r["PORT"] == port), key=lambda r: r["start"])
            for a, b in zip(mine, mine[1:]):
                self.assertLessEqual(a["end"], b["start"] + 0.05)
        # `long` and `mid` start together, one in each lane.
        self.assertNotEqual(records["long"]["PORT"], records["mid"]["PORT"])

    def test_one_lane_takes_the_recipes_longest_first(self):
        done = self.run_recipes("e14-builds e14-bad e14-short e14-survivor", lanes=1)
        self.assertEqual(done.returncode, 1)
        by_start = [r["name"] for r in sorted(self.records().values(), key=lambda r: r["start"])]
        self.assertEqual(by_start, ["short", "bad", "survivor", "builds"])

    def test_a_signal_stops_the_lane_starts_nothing_more_and_leaves_nothing(self):
        ports = free_range(1)
        proc = subprocess.Popen(
            [sys.executable, "scripts/preverify.py", *NO_DISK_BOUND, "--recipes", "e14-hang e14-short e14-bad",
             "--lanes", "1", "--no-setup", "--ports", ports, "--out", str(self.out), "--cpus", "8"],
            cwd=self.toy, env=self.env(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.addCleanup(self.reap, proc)
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline and not (self.seen / "hang.json").exists():
            time.sleep(0.1)
        self.assertTrue((self.seen / "hang.json").exists(), "the recipe did not start")
        hung = json.loads((self.seen / "hang.json").read_text())["pid"]
        self.assertEqual((self.out / "pid").read_text().strip(), str(proc.pid))
        started = time.monotonic()
        proc.send_signal(signal.SIGTERM)
        out, err = proc.communicate(timeout=60)
        self.assertLess(time.monotonic() - started, 45)
        self.assertEqual(proc.returncode, 1, out + err)
        self.assertFalse(pv.alive(hung), "the recipe still runs after the signal")
        self.assertEqual(set(self.records()), {"hang"}, "a recipe was started after the signal")
        self.assertEqual(len(self.worktrees()), 1, "a lane was left registered")
        self.assertFalse((self.out / "lane-0" / "tree").exists())
        self.assertFalse((self.out / "pid").exists())
        results = {r["recipe"]: r for r in json.loads((self.out / "summary.json").read_text())["results"]}
        self.assertEqual(results["e14-hang"]["reason"], "interrupted")
        self.assertEqual(results["e14-short"]["reason"], "not run: interrupted")
        self.assertEqual(results["e14-bad"]["verdict"], "NOT RUN")
        self.assertIn("- e14-hang: interrupted", out)


class Setup(Toy):
    """A lane sets up as a shard does: the node workspace, and the build once
    a recipe needs it. Run in this process, so that the steps can be toys."""

    def run_in_process(self, recipes, lanes, bootstrap, build, env_extra=None, extra=()):
        ports = free_range(lanes)
        self.addCleanup(self.drop_lock, ports)
        argv = ["--recipes", recipes, "--lanes", str(lanes), "--ports", ports, "--out", str(self.out), "--cpus", "8",
                *extra]
        buffer = io.StringIO()
        env = self.env(**(env_extra or {}))
        with mock.patch.dict(os.environ, env, clear=True), \
                mock.patch.object(pv, "BOOTSTRAP_STEPS", bootstrap), \
                mock.patch.object(pv, "BUILD_STEPS", build), \
                redirect_stdout(buffer):
            code = pv.main(argv, self.toy)
        return code, buffer.getvalue()

    def test_every_lane_installs_and_only_a_lane_that_needs_the_build_builds(self):
        marks = self.temp / "marks"
        marks.mkdir()
        bootstrap = [f'echo "$PORT" > {marks}/bootstrap-$PORT']
        build = [f'echo "$PORT" > {marks}/build-$PORT', f'test -d {self.toy}']
        code, out = self.run_in_process("e14-builds e14-long", 2, bootstrap, build)
        self.assertEqual(code, 0, out)
        names = sorted(p.name for p in marks.iterdir())
        stride, _ = pv.mutation_slots.port_layout(ROOT)
        self.assertEqual(len([n for n in names if n.startswith("bootstrap-")]), 2, names)
        builds = [n for n in names if n.startswith("build-")]
        self.assertEqual(len(builds), 1, names)
        builder = int(builds[0].split("-")[1])
        self.assertEqual(self.records()["builds"]["PORT"], str(builder))
        self.assertNotEqual(self.records()["long"]["PORT"], str(builder))

    def test_a_survivor_in_a_running_recipes_evidence_is_said_while_it_runs(self):
        # Evidence committed with the tree, a survivor among it, is not news.
        (self.toy / "docs/evidence/E14").mkdir(parents=True)
        (self.toy / "docs/evidence/E14/committed.txt").write_text("an old mutant: SURVIVED\n")
        git(self.toy, "add", "-A")
        git(self.toy, "commit", "-q", "-m", "evidence that was already there")
        with mock.patch.object(pv, "GLANCE_EVERY", 0.3):
            # a setup that takes a moment, as a real one does: the tree is older than the recipe
            code, out = self.run_in_process("e14-early", 1, ["sleep 0.3"], [])
        self.assertEqual(code, 1, out)
        said = out.index("lane 0: e14-early so far: an early mutant: SURVIVED   (early.txt)")
        ended = out.index("lane 0: e14-early FAILED")
        self.assertLess(said, ended, "said at the recipe's end, not while it ran")
        self.assertNotIn("a first mutant", out.split("lane 0: e14-early FAILED")[0], "a killed mutant is no sign")
        self.assertNotIn("committed.txt", out, "evidence the tree carried was taken for the recipe's")
        self.assertIn("| `e14-early` | FAILED |", out)

    def test_a_lane_told_to_skip_setup_runs_none_of_it(self):
        marks = self.temp / "marks"
        marks.mkdir()
        code, out = self.run_in_process(
            "e14-builds", 1, [f"echo x > {marks}/boot"], [f"echo x > {marks}/build"], extra=["--no-setup"])
        self.assertEqual(code, 0, out)
        self.assertEqual(sorted(p.name for p in marks.iterdir()), [])
        self.assertEqual(set(self.records()), {"builds"})

    def test_a_lane_builds_once_however_many_recipes_need_it(self):
        marks = self.temp / "marks"
        marks.mkdir()
        (self.toy / "justfile").write_text(JUSTFILE + "e14-builds-too:\n    @python3 scripts/toy.py builds-too 0.1 ok target/debug/pw\n")
        git(self.toy, "commit", "-q", "-am", "a second recipe that builds")
        code, out = self.run_in_process("e14-builds e14-builds-too", 1, [], [f'echo x >> {marks}/builds'])
        self.assertEqual(code, 0, out)
        self.assertEqual((marks / "builds").read_text().count("x"), 1)
        self.assertEqual(set(self.records()), {"builds", "builds-too"})

    def test_the_steps_run_in_the_lanes_tree_in_order_and_with_its_environment(self):
        marks = self.temp / "marks"
        marks.mkdir()
        code, out = self.run_in_process(
            "e14-builds", 1,
            [f'pwd > {marks}/pwd; echo "$CARGO_TARGET_DIR $TMPDIR" > {marks}/env; echo boot >> {marks}/order; '
             f'echo "[$PW_FEED_DATABASE_URL]" > {marks}/db'],
            [f'echo build >> {marks}/order'])
        self.assertEqual(code, 0, out)
        self.assertEqual((marks / "db").read_text().strip(), "[]", "a setup command was given the database")
        self.assertEqual((marks / "order").read_text().split(), ["boot", "build"])
        tree = pathlib.Path((marks / "pwd").read_text().strip())
        self.assertEqual(tree.resolve(), (self.out / "lane-0" / "tree").resolve())
        self.assertEqual((marks / "env").read_text().split(), [str(self.out / "lane-0" / "tree" / "target"), str(self.out / "lane-0" / "tmp")])
        log = (self.out / "lane-0" / "setup.log").read_text()
        self.assertIn("$ pwd > ", log)
        self.assertIn("$ echo build >> ", log)
        self.assertEqual(log.count("(exit 0, done)"), 2)

    def test_a_lane_whose_install_fails_stops_and_says_so_while_another_runs_everything(self):
        ports = free_range(2)
        first = ports.split("-")[0]
        argv = ["--recipes", "e14-short e14-feed-postgres e14-builds", "--lanes", "2", "--ports", ports,
                "--out", str(self.out), "--cpus", "8"]
        buffer = io.StringIO()
        with mock.patch.dict(os.environ, self.env(), clear=True), \
                mock.patch.object(pv, "BOOTSTRAP_STEPS", [f'test "$PORT" != "{first}"']), \
                mock.patch.object(pv, "BUILD_STEPS", []), redirect_stdout(buffer):
            code = pv.main(argv, self.toy)
        out = buffer.getvalue()
        self.assertEqual(code, 1, out)
        results = json.loads((self.out / "summary.json").read_text())
        self.assertEqual({r["recipe"]: r["verdict"] for r in results["results"]},
                         {"e14-short": "ok", "e14-feed-postgres": "ok", "e14-builds": "ok"})
        self.assertEqual({r["lane"] for r in results["results"]}, {1})
        self.assertIn("- lane 0 stopped: `test", out)
        self.assertIn("lane 0: stopped,", out)

    def test_a_failed_build_stops_the_lane_and_leaves_its_recipe_to_another(self):
        ports = free_range(2)
        first = ports.split("-")[0]
        # (e14-builds needs the build; the lane on the first port cannot build.)
        (self.toy / "justfile").write_text(JUSTFILE + "e14-builds-too:\n    @python3 scripts/toy.py builds-too 0.1 ok target/debug/pw\n")
        git(self.toy, "commit", "-q", "-am", "a second recipe that builds")
        argv = ["--recipes", "e14-builds e14-builds-too", "--lanes", "2", "--ports", ports, "--out", str(self.out),
                "--cpus", "8"]
        buffer = io.StringIO()
        with mock.patch.dict(os.environ, self.env(), clear=True), \
                mock.patch.object(pv, "BOOTSTRAP_STEPS", []), \
                mock.patch.object(pv, "BUILD_STEPS", [f'test "$PORT" != "{first}"']), redirect_stdout(buffer):
            code = pv.main(argv, self.toy)
        out = buffer.getvalue()
        self.assertEqual(code, 1, out)
        results = json.loads((self.out / "summary.json").read_text())
        self.assertEqual({r["recipe"]: r["verdict"] for r in results["results"]},
                         {"e14-builds": "ok", "e14-builds-too": "ok"})
        self.assertEqual({r["lane"] for r in results["results"]}, {1})
        self.assertIn("lane 0 stopped: `test", out)

    def test_when_no_lane_can_set_up_what_is_left_is_not_run(self):
        code, out = self.run_in_process("e14-short e14-builds", 2, ["false"], [])
        self.assertEqual(code, 1, out)
        results = json.loads((self.out / "summary.json").read_text())
        self.assertEqual({r["recipe"]: r["verdict"] for r in results["results"]},
                         {"e14-short": "NOT RUN", "e14-builds": "NOT RUN"})
        self.assertIn("not run: no lane could run it", out)
        self.assertEqual(self.records(), {})

    def test_the_toolchain_is_copied_into_the_lanes_and_not_downloaded(self):
        (self.toy / ".toolchain" / "prefix" / "bin" / "koka").write_text("#!/bin/sh\necho koka\n")
        marks = self.temp / "marks"
        marks.mkdir()
        code, out = self.run_in_process(
            "e14-short", 1, [f'ls .toolchain/prefix/bin > {marks}/seen; test ! -L .toolchain'], [])
        self.assertEqual(code, 0, out)
        self.assertEqual((marks / "seen").read_text().split(), ["koka"])
        # and the original is the original
        self.assertEqual((self.toy / ".toolchain/prefix/bin/koka").read_text(), "#!/bin/sh\necho koka\n")

    def test_a_run_says_what_the_lanes_are_doing_every_so_often(self):
        with mock.patch.object(pv, "STATUS_EVERY", 0.4):
            code, out = self.run_in_process("e14-long", 1, [], [])
        self.assertEqual(code, 0, out)
        self.assertRegex(out, r"\[\d\d:\d\d:\d\d\] running: lane 0 e14-long \(\d+ s\); 0 of 1 recipes done, 0 failed; load [\d.]+")

    def test_the_callers_signal_handlers_are_put_back(self):
        before = (signal.getsignal(signal.SIGINT), signal.getsignal(signal.SIGTERM))
        code, out = self.run_in_process("e14-short", 1, [], [])
        self.assertEqual(code, 0, out)
        self.assertEqual((signal.getsignal(signal.SIGINT), signal.getsignal(signal.SIGTERM)), before)

    def test_a_fault_in_the_tool_is_a_recipes_failure_said_not_a_lost_result(self):
        with mock.patch.object(pv.Lane, "attempt", side_effect=RuntimeError("boom")):
            code, out = self.run_in_process("e14-short e14-builds", 1, [], [])
        self.assertEqual(code, 1, out)
        results = json.loads((self.out / "summary.json").read_text())["results"]
        self.assertEqual(
            {r["recipe"]: (r["verdict"], r["reason"]) for r in results},
            {"e14-short": ("FAILED", "preverify failed: RuntimeError('boom')"),
             "e14-builds": ("FAILED", "preverify failed: RuntimeError('boom')")})
        self.assertIn("- e14-short: preverify failed: RuntimeError('boom')", out)

    def test_a_fault_that_ends_a_lane_is_said_and_the_others_go_on(self):
        real = pv.Lane.bootstrap

        def faulty(lane):
            if lane.index == 0:
                raise RuntimeError("no disk")
            return real(lane)

        with mock.patch.object(pv.Lane, "bootstrap", faulty):
            code, out = self.run_in_process("e14-short e14-builds e14-feed-postgres", 2, [], [])
        self.assertEqual(code, 1, out)
        results = json.loads((self.out / "summary.json").read_text())["results"]
        self.assertEqual({r["recipe"]: r["verdict"] for r in results},
                         {"e14-short": "ok", "e14-builds": "ok", "e14-feed-postgres": "ok"})
        self.assertEqual({r["lane"] for r in results}, {1})
        self.assertIn("lane 0: stopped, preverify failed: RuntimeError('no disk')", out)
        self.assertIn("- lane 0 stopped: preverify failed: RuntimeError('no disk')", out)


# ---------------------------------------------------------------------------
# Judging and reporting
# ---------------------------------------------------------------------------


def recipe_out(root: pathlib.Path, name: str, status: int, evidence: dict[str, str], log: str = "") -> pathlib.Path:
    """What the runner leaves of a recipe: results.json, its log, the evidence."""
    out = root / name
    (out / "logs").mkdir(parents=True)
    (out / "logs" / f"{name}.log").write_text(log)
    for path, text in evidence.items():
        target = out / "evidence" / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    (out / "results.json").write_text(json.dumps([
        {"recipe": name, "status": status, "seconds": 12.5, "wrote": sorted(evidence), "free": "9 GiB"}]))
    return out


class Judging(unittest.TestCase):
    def setUp(self):
        self.temp = pathlib.Path(tempfile.mkdtemp(prefix="pv-judge-"))
        self.addCleanup(shutil.rmtree, self.temp, True)

    def test_a_recipe_passes_on_a_zero_exit_with_no_survivor(self):
        self.assertTrue(pv.judge(0, []))
        self.assertFalse(pv.judge(1, []))
        self.assertFalse(pv.judge(0, ["x: SURVIVED"]))
        self.assertFalse(pv.judge(None, []))
        self.assertFalse(pv.judge(101, ["x: SURVIVED"]))

    def test_a_survivor_is_a_line_that_says_survived_or_an_anchor_gone(self):
        text = "a: KILLED\nb: SURVIVED\n  c: ANCHOR NOT FOUND EXACTLY ONCE in x.py\nd: killed\n"
        self.assertEqual(pv.survivors(text), ["b: SURVIVED", "c: ANCHOR NOT FOUND EXACTLY ONCE in x.py"])
        self.assertEqual(pv.survivors("all killed\n"), [])

    def test_the_memory_bound_lines_are_listed_but_not_the_untouched_one(self):
        text = "memory bound: no process was stopped\nmemory bound: stopped `cargo`\nother\n"
        self.assertEqual(pv.bound_lines("e14-x", text), ["e14-x: memory bound: stopped `cargo`"])

    def test_a_result_is_read_from_the_runners_record_and_its_evidence(self):
        out = recipe_out(self.temp, "e14-a", 0, {"docs/evidence/E14/a.txt": "ok\n"})
        result = pv.result_of("e14-a", 3, out, "done", [])
        self.assertEqual((result.verdict, result.lane, result.seconds, result.reason), ("ok", 3, 12.5, ""))
        self.assertEqual(result.wrote, ["docs/evidence/E14/a.txt"])
        self.assertEqual(result.out, str(out))

    def test_evidence_the_runner_wrote_and_did_not_copy_fails_a_recipe(self):
        out = recipe_out(self.temp, "e14-a", 0, {"docs/evidence/E14/a.txt": "ok\n"})
        (out / "evidence" / "docs/evidence/E14/a.txt").unlink()
        result = pv.result_of("e14-a", 0, out, "done", [])
        self.assertEqual(result.verdict, "FAILED")
        self.assertEqual(result.reason, "docs/evidence/E14/a.txt was written, and the runner's copy of it is missing")

    def test_a_non_zero_exit_is_failed_and_says_the_exit(self):
        out = recipe_out(self.temp, "e14-a", 101, {"docs/evidence/E14/a.txt": "ok\n"})
        result = pv.result_of("e14-a", 0, out, "done", [])
        self.assertEqual((result.verdict, result.reason, result.status), ("FAILED", "exit 101", 101))

    def test_a_survivor_fails_a_recipe_and_the_first_three_are_the_reason(self):
        text = "".join(f"m{n}: SURVIVED\n" for n in range(5))
        out = recipe_out(self.temp, "e14-a", 0, {"docs/evidence/E14/a.txt": text})
        result = pv.result_of("e14-a", 0, out, "done", [])
        self.assertEqual(result.verdict, "FAILED")
        self.assertEqual(result.reason, "m0: SURVIVED; m1: SURVIVED; m2: SURVIVED")
        self.assertEqual(len(result.survived), 5)

    def test_an_exit_wins_over_survivors_in_the_reason(self):
        out = recipe_out(self.temp, "e14-a", 2, {"docs/evidence/E14/a.txt": "m: SURVIVED\n"})
        self.assertEqual(pv.result_of("e14-a", 0, out, "done", []).reason, "exit 2")

    def test_no_record_is_a_failure_saying_why(self):
        out = self.temp / "e14-a"
        out.mkdir()
        self.assertEqual(pv.result_of("e14-a", 0, out, "timeout", []).reason, "timed out")
        self.assertEqual(pv.result_of("e14-a", 0, out, "stopped", []).reason, "interrupted")
        self.assertEqual(pv.result_of("e14-a", 0, out, "done", []).reason, "the runner recorded no result")
        self.assertEqual(pv.result_of("e14-a", 0, out, "done", []).verdict, "FAILED")

    def test_processes_left_behind_are_a_note_not_a_failure(self):
        out = recipe_out(self.temp, "e14-a", 0, {"docs/evidence/E14/a.txt": "ok\n"})
        result = pv.result_of("e14-a", 0, out, "done", [1, 2])
        self.assertEqual(result.verdict, "ok")
        self.assertEqual(result.notes, ["left 2 process(es) running, stopped after it"])

    def test_a_recipe_not_run_is_said_not_run(self):
        result = pv.not_run("e14-a", "because")
        self.assertEqual((result.verdict, result.reason, result.lane), ("NOT RUN", "not run: because", None))


class FailingLines(unittest.TestCase):
    def test_what_cargo_says_of_a_failed_test(self):
        text = (
            "running 3 tests\ntest a::ok ... ok\ntest a::bad ... FAILED\n\nfailures:\n\n---- a::bad stdout ----\n"
            "thread 'a::bad' panicked at src/a.rs:4:5:\nassertion failed\n\nfailures:\n    a::bad\n\n"
            "test result: FAILED. 2 passed; 1 failed; 0 ignored\n"
        )
        self.assertEqual(pv.failing_lines(text), [
            "test a::bad ... FAILED", "---- a::bad stdout ----", "thread 'a::bad' panicked at src/a.rs:4:5:",
            "test result: FAILED. 2 passed; 1 failed; 0 ignored",
        ])

    def test_what_playwright_says(self):
        text = (
            "  1) [webkit] › e2e/messages.spec.mjs:232:5 › a message is read\n\n    Error: expect(received)\n\n"
            "  ✘  2 [firefox] › e2e/x.spec.mjs:3 › y (1.2s)\n  1 failed\n  3 flaky\n  12 passed\n  0 failed\n"
        )
        found = pv.failing_lines(text)
        self.assertEqual(found[0], "1) [webkit] › e2e/messages.spec.mjs:232:5 › a message is read")
        self.assertIn("✘  2 [firefox] › e2e/x.spec.mjs:3 › y (1.2s)", found)
        self.assertIn("1 failed", found)
        self.assertIn("3 flaky", found)
        self.assertNotIn("12 passed", found)
        self.assertNotIn("0 failed", found)

    def test_what_unittest_and_a_mutation_script_say(self):
        text = (
            "FAIL: test_x (test_y.Z)\nERROR: test_w (test_y.Z)\nsome mutant: SURVIVED\n"
            "other mutant: ANCHOR NOT FOUND EXACTLY ONCE in a.py\nbaseline (core): 3 passed, 2 failed\n"
            "baseline (core): 3 passed, 0 failed\nFAIL: the unmutated baseline is not green; no mutant can mean anything\n"
            "error[E0425]: cannot find value `x` in this scope\n    Error: expect(received).toBe(expected)\nall fine\n"
        )
        found = pv.failing_lines(text)
        self.assertEqual(len(found), 8)
        self.assertIn("Error: expect(received).toBe(expected)", found)
        self.assertNotIn("baseline (core): 3 passed, 0 failed", found)
        self.assertNotIn("all fine", found)

    def test_colours_are_taken_out_lines_are_once_each_and_the_list_is_capped(self):
        self.assertEqual(pv.failing_lines("\x1b[31mtest a ... FAILED\x1b[0m\ntest a ... FAILED\n"), ["test a ... FAILED"])
        self.assertEqual(pv.failing_lines("\x1b[1;31mtest b ... FAILED\x1b[0m\n"), ["test b ... FAILED"])
        many = "".join(f"test t{n} ... FAILED\n" for n in range(100))
        self.assertEqual(len(pv.failing_lines(many)), 30)
        self.assertEqual(len(pv.failing_lines(many, limit=5)), 5)
        self.assertEqual(pv.failing_lines("nothing\n"), [])


class Report(unittest.TestCase):
    def setUp(self):
        self.temp = pathlib.Path(tempfile.mkdtemp(prefix="pv-report-"))
        self.addCleanup(shutil.rmtree, self.temp, True)
        self.jobs = [
            pv.Job("e14-a", 100, 100.0, False, False, False),
            pv.Job("e14-b", 50, None, True, True, False),
            pv.Job("e14-c", 10, 10.0, False, False, True),
        ]
        self.meta = {"head": "a" * 40, "base": "b" * 40, "lanes": 2, "cpus": 32, "sample": 30, "wall": 600.0,
                     "load_start": 10.0, "load_end": 20.0, "load_max": 30.5}

    def results(self):
        ok = recipe_out(self.temp, "e14-a", 0, {"docs/evidence/E14/a.txt": "fine\n"})
        bad = recipe_out(
            self.temp, "e14-b", 101, {"docs/evidence/E14/b.txt": "test x ... FAILED\nlast evidence line\n"},
            log="a log line\ntest y ... FAILED\nlast log line\n")
        return [
            pv.result_of("e14-a", 0, ok, "done", []),
            pv.result_of("e14-b", 1, bad, "done", []),
            pv.not_run("e14-c", "no lane could run it"),
        ]

    def test_the_summary_has_the_count_the_failures_and_the_table(self):
        text = pv.render(self.results(), self.jobs, self.meta, [])
        self.assertTrue(text.startswith("**3 recipes, 2 failed**\n\n- e14-b: exit 101\n- e14-c: not run: no lane could run it\n"))
        self.assertIn("| recipe | result | seconds | lane | CI seconds | files |", text)
        self.assertIn("| `e14-a` | ok | 12.5 | 0 | 100 | 1 |", text)
        self.assertIn("| `e14-b` | FAILED | 12.5 | 1 |  | 1 |", text)
        self.assertIn("| `e14-c` | NOT RUN | 0 |  | 10 | 0 |", text)

    def test_the_summary_is_sorted_by_recipe_as_cis_is(self):
        rows = re.findall(r"^\| `(e14-[a-z])`", pv.render(list(reversed(self.results())), self.jobs, self.meta, []), re.M)
        self.assertEqual(rows, ["e14-a", "e14-b", "e14-c"])

    def test_the_totals_say_the_wall_clock_the_load_and_ci_seconds(self):
        text = pv.render(self.results(), self.jobs, self.meta, [])
        self.assertIn("pre-check at aaaaaaaaaaaa, changed since bbbbbbbbbbbb: 3 recipes in 2 lanes, wall-clock 600 s (10.0 min)", text)
        self.assertIn("CI recorded 110 s for the 2 it has measured, the longest 100 s", text)
        self.assertIn("load average (1 min): 10.0 at the start, 20.0 at the end, 30.5 at most (sampled every 30 s), on 32 threads", text)

    def test_a_recipe_not_run_has_a_block_of_one_line(self):
        self.assertEqual(pv.failure_block(pv.not_run("e14-c", "no lane could run it")),
                         "--- e14-c: not run: no lane could run it")

    def test_a_run_of_named_recipes_says_so_in_place_of_a_base(self):
        text = pv.render(self.results()[:1], self.jobs, dict(self.meta, base=""), [])
        self.assertIn("pre-check at aaaaaaaaaaaa, recipes named: 1 recipes in 2 lanes", text)

    def test_a_long_log_is_shown_by_its_last_forty_lines(self):
        out = recipe_out(self.temp, "e14-b", 1, {}, log="".join(f"line {n}\n" for n in range(100)))
        block = pv.failure_block(pv.result_of("e14-b", 0, out, "done", []))
        self.assertIn("e14-b: log, its last 40 lines", block)
        self.assertIn("  | line 99\n", block + "\n")
        self.assertIn("  | line 60\n", block)
        self.assertNotIn("  | line 59\n", block)

    def test_a_failure_shows_its_failing_lines_and_the_tail_of_its_log_and_evidence(self):
        text = pv.render(self.results(), self.jobs, self.meta, [])
        block = text.split("--- e14-b: exit 101 (lane 1, 12.5 s)")[1]
        self.assertIn("failing test lines:\n  | test y ... FAILED\n  | test x ... FAILED", block)
        self.assertIn("--- e14-b: log, its last 3 lines", block)
        self.assertIn("  | last log line", block)
        self.assertIn("--- e14-b: docs/evidence/E14/b.txt, its last 2 lines", block)
        self.assertIn("  | last evidence line", block)
        self.assertIn("log: ", block)

    def test_a_run_prints_a_line_to_run_the_failed_ones_again(self):
        results = self.results()
        text = pv.render(results, self.jobs, self.meta, [])
        self.assertIn('to run only these again: just preverify origin/master --recipes "e14-b e14-c"', text)
        clean = pv.render(results[:1], self.jobs, self.meta, [])
        self.assertNotIn("to run only these again", clean)

    def test_a_lane_that_stopped_and_each_lanes_setup_are_said(self):
        setups = [{"lane": 0, "port": 100, "setup": 12.4, "broken": "`cargo build` failed", "recipes": 0},
                  {"lane": 1, "port": 356, "setup": 30.0, "broken": None, "recipes": 2}]
        text = pv.render(self.results()[:1], self.jobs, self.meta, setups)
        self.assertIn("- lane 0 stopped: `cargo build` failed", text)
        self.assertIn("lane 0: PORT 100, setup 12 s, 0 recipes", text)
        self.assertIn("lane 1: PORT 356, setup 30 s, 2 recipes", text)

    def test_what_the_memory_bound_stopped_is_listed_apart(self):
        out = recipe_out(self.temp, "e14-a", 0, {"docs/evidence/E14/a.txt": "memory bound: stopped `cargo`, which held\n"})
        text = pv.render([pv.result_of("e14-a", 0, out, "done", [])], self.jobs, self.meta, [])
        self.assertIn("Invalid producer: memory guard stopped a process or stopped watching; no kill:\n\n- e14-a: memory bound: stopped `cargo`, which held", text)

    def test_processes_a_recipe_left_are_listed_apart(self):
        out = recipe_out(self.temp, "e14-a", 0, {"docs/evidence/E14/a.txt": "fine\n"})
        text = pv.render([pv.result_of("e14-a", 0, out, "done", [7])], self.jobs, self.meta, [])
        self.assertIn("Left running by a recipe, and stopped (CI's runner would have kept them):", text)
        self.assertIn("- e14-a: left 1 process(es) running, stopped after it", text)

    def test_a_clean_summary_has_no_failure_section(self):
        text = pv.render(self.results()[:1], self.jobs, self.meta, [])
        self.assertTrue(text.startswith("**1 recipes, 0 failed**\n\n| recipe |"))
        self.assertNotIn("--- e14", text)


class Processes(unittest.TestCase):
    def spawn(self, marker, detached=True):
        env = dict(os.environ)
        if marker:
            env[pv.MARKER] = marker
        return subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(120)"], env=env, start_new_session=detached,
        )

    def test_a_marked_process_is_found_in_a_session_of_its_own_and_an_unmarked_one_is_not(self):
        marker = f"/pv-test-{os.getpid()}"
        a, b = self.spawn(marker), self.spawn(None)
        self.addCleanup(lambda: [(p.kill(), p.wait()) for p in (a, b)])
        self.assertEqual(pv.marked(marker), [a.pid])
        self.assertEqual(pv.marked(marker + "0"), [])

    def test_a_marker_is_matched_whole_not_as_a_prefix(self):
        marker = f"/pv-test-lane-1-{os.getpid()}"
        a = self.spawn(marker + "0")
        self.addCleanup(lambda: (a.kill(), a.wait()))
        self.assertEqual(pv.marked(marker), [])

    def test_the_marked_are_stopped_and_the_rest_left(self):
        marker = f"/pv-test-stop-{os.getpid()}"
        a, b = self.spawn(marker), self.spawn(None)
        self.addCleanup(lambda: (b.kill(), b.wait()))
        found = pv.stop_marked(marker, grace=5)
        self.assertEqual(found, [a.pid])
        a.wait(timeout=10)
        self.assertEqual(a.returncode, -signal.SIGTERM, "stopped by SIGKILL without the grace")
        self.assertIsNone(b.poll())

    def test_a_process_is_given_the_grace_to_end_cleanly(self):
        marker = f"/pv-test-grace-{os.getpid()}"
        with tempfile.TemporaryDirectory() as t:
            done = pathlib.Path(t) / "done"
            code = (
                "import pathlib, signal, sys, time\n"
                "def stop(*a):\n"
                "    time.sleep(0.5)\n"
                f"    pathlib.Path({str(done)!r}).write_text('clean')\n"
                "    sys.exit(0)\n"
                "signal.signal(signal.SIGTERM, stop)\n"
                "print('up', flush=True)\n"
                "time.sleep(120)\n"
            )
            proc = subprocess.Popen([sys.executable, "-c", code], env={**os.environ, pv.MARKER: marker},
                                    start_new_session=True, stdout=subprocess.PIPE, text=True)
            self.addCleanup(lambda: (proc.kill(), proc.wait(), proc.stdout.close()))
            proc.stdout.readline()
            pv.stop_marked(marker, grace=10)
            proc.wait(timeout=10)
            self.assertEqual(done.read_text(), "clean", "killed before it had ended")
            self.assertEqual(proc.returncode, 0)

    def test_a_term_proof_process_is_killed_after_the_grace(self):
        marker = f"/pv-test-kill-{os.getpid()}"
        env = {**os.environ, pv.MARKER: marker}
        code = "import signal, time; signal.signal(signal.SIGTERM, lambda *a: None); print('up', flush=True); time.sleep(120)"
        proc = subprocess.Popen([sys.executable, "-c", code], env=env, start_new_session=True, stdout=subprocess.PIPE, text=True)
        self.addCleanup(lambda: (proc.kill(), proc.wait(), proc.stdout.close()))
        proc.stdout.readline()
        pv.stop_marked(marker, grace=0.5)
        proc.wait(timeout=10)
        self.assertEqual(proc.returncode, -signal.SIGKILL)

    def test_terminate_stops_a_process_and_its_group(self):
        code = (
            "import subprocess, sys, time\n"
            "c = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'])\n"
            "print(c.pid, flush=True)\ntime.sleep(120)\n"
        )
        proc = subprocess.Popen([sys.executable, "-c", code], start_new_session=True, stdout=subprocess.PIPE, text=True)
        self.addCleanup(proc.stdout.close)
        child = int(proc.stdout.readline())
        self.addCleanup(lambda: os.kill(child, signal.SIGKILL) if pv.alive(child) else None)
        pv.terminate(proc, grace=5)
        self.assertIsNotNone(proc.returncode)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and pv.alive(child):
            time.sleep(0.05)
        self.assertFalse(pv.alive(child), "the group's other process still runs")

    def test_a_term_proof_group_is_killed(self):
        code = "import signal, time; signal.signal(signal.SIGTERM, lambda *a: None); print('up', flush=True); time.sleep(120)"
        proc = subprocess.Popen([sys.executable, "-c", code], start_new_session=True, stdout=subprocess.PIPE, text=True)
        self.addCleanup(proc.stdout.close)
        proc.stdout.readline()
        pv.terminate(proc, grace=0.5)
        self.assertEqual(proc.returncode, -signal.SIGKILL)

    def test_a_zombie_is_not_alive_and_a_pid_that_is_gone_is_not_either(self):
        proc = subprocess.Popen([sys.executable, "-c", "pass"])
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and pv.alive(proc.pid):
            time.sleep(0.05)
        self.assertFalse(pv.alive(proc.pid))
        proc.wait()
        self.assertFalse(pv.alive(proc.pid))
        self.assertTrue(pv.alive(os.getpid()))

    def test_a_watched_process_is_looked_at_every_so_often_and_a_look_that_fails_changes_nothing(self):
        looks = []

        def look():
            looks.append(time.monotonic())
            raise RuntimeError("a look that fails")

        proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(3)"], start_new_session=True)
        self.assertEqual(pv.watch(proc, pv.threading.Event(), 30, look, every=0.5), "done")
        self.assertGreaterEqual(len(looks), 2)
        quiet = subprocess.Popen([sys.executable, "-c", "pass"], start_new_session=True)
        self.assertEqual(pv.watch(quiet, pv.threading.Event(), 30, look, every=0.5), "done")
        rarely = []
        slow = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(2.5)"], start_new_session=True)
        self.assertEqual(pv.watch(slow, pv.threading.Event(), 30, lambda: rarely.append(1), every=100), "done")
        self.assertEqual(rarely, [], "looked at before its time")

    def test_a_watched_process_that_ends_is_done_and_one_that_overstays_is_stopped(self):
        stop = pv.threading.Event()
        quick = subprocess.Popen([sys.executable, "-c", "pass"], start_new_session=True)
        self.assertEqual(pv.watch(quick, stop, 30), "done")
        slow = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(120)"], start_new_session=True)
        self.assertEqual(pv.watch(slow, stop, 0.5), "timeout")
        self.assertIsNotNone(slow.returncode)
        again = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(120)"], start_new_session=True)
        stop.set()
        self.assertEqual(pv.watch(again, stop, 30), "stopped")
        self.assertIsNotNone(again.returncode)


class Glances(unittest.TestCase):
    def setUp(self):
        self.temp = pathlib.Path(tempfile.mkdtemp(prefix="pv-glance-"))
        self.addCleanup(shutil.rmtree, self.temp, True)
        self.evidence = self.temp / "docs" / "evidence" / "E14"
        self.evidence.mkdir(parents=True)
        self.said = []
        self.glance = pv.Glance(self.temp, "e14-x", 2, self.said.append, time.time() - 5)

    def test_a_line_that_says_failed_is_said_once_with_its_lane_recipe_and_file(self):
        (self.evidence / "x.txt").write_text("fine\nm: SURVIVED\n")
        self.glance()
        self.glance()
        self.assertEqual(self.said, ["lane 2: e14-x so far: m: SURVIVED   (x.txt)"])

    def test_what_does_not_say_failed_is_not_said(self):
        (self.evidence / "x.txt").write_text(
            "m: KILLED (1 of 3 tests fail)\nbaseline (a): 3 passed, 0 failed\ntest result: ok. 3 passed; 0 failed\n")
        self.glance()
        self.assertEqual(self.said, [])

    def test_lines_written_later_are_said_and_the_earlier_are_not_said_again(self):
        path = self.evidence / "x.txt"
        path.write_text("a: SURVIVED\n")
        self.glance()
        with open(path, "a") as out:
            out.write("fine\nb: SURVIVED\n")
        self.glance()
        self.assertEqual([m.split(" so far: ")[1].split("   ")[0] for m in self.said], ["a: SURVIVED", "b: SURVIVED"])

    def test_a_line_not_yet_whole_waits_for_its_end(self):
        path = self.evidence / "x.txt"
        path.write_text("a mutant: SURVIV")
        self.glance()
        self.assertEqual(self.said, [])
        with open(path, "a") as out:
            out.write("ED\n")
        self.glance()
        self.assertEqual(len(self.said), 1)
        self.assertIn("a mutant: SURVIVED", self.said[0])

    def test_a_file_written_before_the_recipe_began_is_not_read(self):
        path = self.evidence / "old.txt"
        path.write_text("a: SURVIVED\n")
        long_ago = time.time() - 3600
        os.utime(path, (long_ago, long_ago))
        self.glance()
        self.assertEqual(self.said, [])

    def test_a_file_that_starts_again_is_read_from_its_start(self):
        path = self.evidence / "x.txt"
        path.write_text("a long line of what is fine, and what is fine again\na: SURVIVED\n")
        self.glance()
        path.write_text("b: SURVIVED\n")
        self.glance()
        self.assertEqual(len(self.said), 2)
        self.assertIn("b: SURVIVED", self.said[1])

    def test_only_text_files_under_the_evidence_directory_are_read(self):
        (self.evidence / "x.json").write_text("a: SURVIVED\n")
        (self.temp / "docs" / "other.txt").write_text("a: SURVIVED\n")
        (self.evidence.parent / "nested").mkdir()
        (self.evidence.parent / "nested" / "deep.txt").write_text("c: SURVIVED\n")
        self.glance()
        self.assertEqual(len(self.said), 1)
        self.assertIn("(deep.txt)", self.said[0])

    def test_after_five_lines_it_says_there_are_more_and_then_nothing(self):
        (self.evidence / "x.txt").write_text("".join(f"m{n}: SURVIVED\n" for n in range(9)))
        self.glance()
        (self.evidence / "y.txt").write_text("another: SURVIVED\n")
        self.glance()
        self.assertEqual(len(self.said), 6)
        self.assertEqual(sum("SURVIVED" in m for m in self.said), 5)
        self.assertIn("more lines like these are in its evidence", self.said[5])

    def test_a_tree_with_no_evidence_yet_is_no_fault(self):
        shutil.rmtree(self.temp / "docs")
        self.glance()
        self.assertEqual(self.said, [])


class ExitStatus(unittest.TestCase):
    def test_zero_only_where_every_recipe_passed_every_lane_stood_and_nothing_stopped_the_run(self):
        ok = [pv.Result("a", "ok")]
        self.assertEqual(pv.exit_status(ok, [], False), 0)
        self.assertEqual(pv.exit_status(ok, [{"broken": None}], False), 0)
        self.assertEqual(pv.exit_status([pv.Result("a", "FAILED")], [], False), 1)
        self.assertEqual(pv.exit_status([pv.not_run("a", "x")], [], False), 1)
        self.assertEqual(pv.exit_status(ok, [{"broken": "x"}], False), 1)
        self.assertEqual(pv.exit_status(ok, [], True), 1)


class Say(Toy):
    def test_the_stamp_counts_hours_minutes_and_seconds_since_the_start(self):
        run = pv.Run(self.toy, "h" * 40, self.out)
        buffer = io.StringIO()
        with mock.patch.object(pv.time, "monotonic", return_value=run.start + 3600 + 2 * 60 + 5.4), \
                redirect_stdout(buffer):
            run.say("hello")
        self.assertEqual(buffer.getvalue(), "[01:02:05] hello\n")


class LaneUnits(Toy):
    def lane(self):
        self.out.mkdir(parents=True, exist_ok=True)
        run = pv.Run(self.toy, git(self.toy, "rev-parse", "HEAD").stdout.strip(), self.out)
        return pv.Lane(run, 0, 40000, 4, None, 60.0, setup=False)

    def test_a_lane_that_exists_is_refused_not_overwritten(self):
        lane = self.lane()
        lane.create()
        self.addCleanup(lane.remove)
        with self.assertRaises(pv.Refusal) as caught:
            self.lane().create()
        self.assertIn("cannot make lane 0's worktree", str(caught.exception))

    def test_a_worktree_git_does_not_know_is_still_taken_away(self):
        lane = self.lane()
        lane.tree.mkdir(parents=True)
        (lane.tree / "x").write_text("x")
        lane.tmp.mkdir()
        lane.remove()
        self.assertFalse(lane.tree.exists())
        self.assertFalse(lane.tmp.exists())

    def test_the_toolchain_is_hard_linked_where_it_can_be(self):
        (self.toy / ".toolchain/prefix/bin/koka").write_text("koka")
        lane = self.lane()
        lane.create()
        self.addCleanup(lane.remove)
        lane.bootstrap()
        theirs = lane.tree / ".toolchain/prefix/bin/koka"
        self.assertEqual(theirs.read_text(), "koka")
        self.assertEqual(os.stat(theirs).st_ino, os.stat(self.toy / ".toolchain/prefix/bin/koka").st_ino)

    def test_a_toolchain_that_cannot_be_linked_is_copied(self):
        (self.toy / ".toolchain/prefix/bin/koka").write_text("koka")
        real = subprocess.run

        def refuse_cp(command, *args, **kwargs):
            if command[0] == "cp":
                return subprocess.CompletedProcess(command, 1, b"", b"cross-device link")
            return real(command, *args, **kwargs)

        lane = self.lane()
        lane.create()
        self.addCleanup(lane.remove)
        with mock.patch.object(pv.subprocess, "run", side_effect=refuse_cp):
            lane.bootstrap()
        theirs = lane.tree / ".toolchain/prefix/bin/koka"
        self.assertEqual(theirs.read_text(), "koka")
        self.assertNotEqual(os.stat(theirs).st_ino, os.stat(self.toy / ".toolchain/prefix/bin/koka").st_ino)

    def test_a_lane_with_no_toolchain_to_copy_sets_up_all_the_same(self):
        shutil.rmtree(self.toy / ".toolchain")
        lane = self.lane()
        lane.create()
        self.addCleanup(lane.remove)
        lane.bootstrap()
        self.assertFalse((lane.tree / ".toolchain").exists())


class Glancing(unittest.TestCase):
    def test_a_running_recipes_evidence_is_looked_at_every_fifteen_seconds_for_five_lines(self):
        self.assertEqual((pv.GLANCE_EVERY, pv.GLANCE_LINES), (15.0, 5))


class Status(unittest.TestCase):
    def test_the_status_says_what_each_lane_runs_and_how_far_the_queue_is(self):
        lanes = [SimpleNamespace(index=0, running=("e14-a", 100.0)), SimpleNamespace(index=1, running=None),
                 SimpleNamespace(index=2, running=("e14-b", 250.0))]
        results = [pv.Result("e14-x", "ok"), pv.Result("e14-y", "FAILED"), pv.not_run("e14-z", "x")]
        self.assertEqual(
            pv.status_line(lanes, results, 5, 41.26, 400.0),
            "running: lane 0 e14-a (300 s), lane 2 e14-b (150 s); 3 of 5 recipes done, 2 failed; load 41.3")
        self.assertEqual(pv.status_line([], [], 3, 1.0, 0.0), "running: nothing; 0 of 3 recipes done, 0 failed; load 1.0")


class Load(unittest.TestCase):
    def test_the_load_is_sampled_at_the_start_and_at_the_end_whatever_happens_between(self):
        values = iter([1.0, 5.0] + [2.0] * 100)
        with mock.patch.object(pv.os, "getloadavg", side_effect=lambda: (next(values), 0, 0)):
            with pv.Load(every=1000) as load:
                pass
        self.assertEqual(load.samples, [1.0, 5.0])

    def test_the_load_is_sampled_while_a_run_goes_on_and_at_both_ends(self):
        values = iter([1.0, 5.0] + [2.0] * 1000)
        with mock.patch.object(pv.os, "getloadavg", side_effect=lambda: (next(values), 0, 0)):
            with pv.Load(every=0.05) as load:
                time.sleep(0.2)
        self.assertEqual(load.samples[0], 1.0)
        self.assertGreaterEqual(len(load.samples), 3)
        self.assertEqual(max(load.samples), 5.0)


class Help(unittest.TestCase):
    def test_the_help_says_it_is_a_pre_check_and_cis_verify_is_the_record(self):
        text = " ".join(pv.parser().format_help().split())
        self.assertIn("A pre-check only: it writes no evidence into this tree and never pushes", text)
        self.assertIn("go under /tmp/preverify-<head>/", text)
        self.assertIn("CI's verify stays the evidence of record (ADR-0281)", text)
        self.assertIn("origin/master", text)

    def test_the_module_says_so_too(self):
        text = " ".join(pv.__doc__.split())
        self.assertIn("A pre-check, not evidence.", text)
        self.assertIn("never writes evidence into the tree it is run in and never pushes", text)
        self.assertIn("CI's `verify` stays the evidence of record (ADR-0281)", text)

    def test_the_default_base_is_origin_master_and_four_lanes(self):
        args = pv.parser().parse_args([])
        self.assertEqual((args.base, args.lanes), ("origin/master", 4))
        self.assertTrue(args.current)
        self.assertIsNone(args.stale)

    def test_the_defaults_are_a_jobs_limit_eight_gib_a_lane_and_the_range_for_lanes(self):
        args = pv.parser().parse_args([])
        self.assertEqual(args.timeout, float(pv.ci_plan.LIMIT))
        self.assertEqual(args.min_free_gib, 8.0)
        self.assertEqual(args.ports, "12741-14740")
        self.assertEqual(args.cpus, os.cpu_count() or 32)
        self.assertEqual((args.keep_lanes, args.no_setup, args.plan, args.recipes, args.out), (False, False, False, [], None))

    def test_the_recipe_passes_its_arguments_through(self):
        text = (ROOT / "just/preverify.just").read_text()
        self.assertRegex(text, r'(?m)^preverify \*args:\n    @python3 scripts/preverify\.py "\$@"$')

    def test_the_tool_reads_nothing_from_github_and_names_no_repository(self):
        """The repository moved (2026-10-20): what is planned comes from the
        tree's refs, and a slug written here would go stale again."""
        for path in ("scripts/preverify.py", "just/preverify.just"):
            text = (ROOT / path).read_text()
            self.assertNotRegex(text, r"(?i)github\.com", path)
            self.assertNotRegex(text, r"\bgh\s+(api|run|pr|repo|workflow|auth)\b", path)
            self.assertNotRegex(text, r"[\w.-]+/perfect-web\b", path)
            self.assertNotIn("--repo", text, path)

    def test_the_ports_option_is_a_range_in_order(self):
        self.assertEqual(pv.port_range("12741-14740"), (12741, 14740))
        self.assertEqual(pv.port_range("5-5"), (5, 5))
        self.assertEqual(pv.port_range("1-65535"), (1, 65535))
        for bad in ("12741", "14740-12741", "1-70000", "a-b", "1-2-3"):
            with self.assertRaises(pv.Refusal):
                pv.port_range(bad)


if __name__ == "__main__":
    unittest.main()
