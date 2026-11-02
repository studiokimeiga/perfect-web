#!/usr/bin/env python3
"""Mutation controls for NEXT 4c's first step (track `render-merge`): within
one render the host makes each identical operation call once, the
operation and its arguments as the key, every binding that asks given the
same answer; the render's calls read one snapshot of the data layer, and
what it takes of another's only where nothing invalidated it since the
render began (the integrator's ruling 3); and on PostgreSQL a render reads
in one of a bounded few seats, every one given back (ruling 2).

Each mutant undoes one rule:

- the merge (`render_merge.rs`): a kept answer never given; the key without
  the arguments, or without the session; a failure kept; nothing ended with
  the render; a render begun inside another its own; a render answering
  for another layer; a render's queries each reading as a query of their
  own; a render taking no snapshot, and no seat;
- the host (`main.rs`): the drain's count and its documents two renders;
  each document read again a render of its own; a page's bindings not one
  render; a call answered again counted as made (`/metrics`); the
  platform's calls merged (the clock's, which note what a read holds
  until); a render reading as any reader does what was invalidated since
  it began; a render held through its telling (the drain's, a session's
  documents'), or begun before its documents are taken (the drain's, a
  session's documents'); a document, or a session, said unshowable inside
  the render;
- the seats (`feed_pg.rs`): a render seated past the bound; a seat never
  given back; an idle connection never taken again; a render's
  transaction ended after every read, or kept after a read that failed; a
  refused seat read in; the store's and the feed's renders reading outside
  their seats;
- the seats a deployment's settings (the integrator's answer of
  2026-10-17): a layer seating to the default's bound, or waiting a test
  host's time, whatever its deployment said; a deployment's count of seats,
  or its seat wait, not read; no seats, or no seconds, read as a count; an
  empty count refused;
- the table's and a turn's checks a test's alone, not every debug build's
  (`debug`: the development server's binary, as `cargo build` makes it,
  carries each check's message where the check is compiled in);
- the snapshot in memory: the store's and the feed's renders reading the
  data as it is now;
- a read as of an epoch (`pw-resource`): each of the four invalidations
  leaving its resource unstamped; a read as of an epoch never alone, or
  alone as of its resource's last invalidation; a read alone taking what is
  kept or a request running, registered as the key's request, or kept.

A mutant must fail the suite it names: `memory`, the merge's own tests and
the served ones, the store in memory (`render_merge::`); `postgres`, the
served ones with the store on PostgreSQL, and NEXT 4b's counts
(`render_merge::`, `tests::bounded_statements::`), which needs
PW_STORE_DATABASE_URL and PW_FEED_DATABASE_URL; `resource`, pw-resource's
`read_as_of` tests; `unit`, the merge's own tests alone; `bound`, the
bound's own test alone (each of the last two for a control the whole suite
cannot run safely: it says why); `debug`, the development server's debug
build, its checks read from its binary (`debug_checks`). Each other suite's
result is read by `cargo_test_results` (ADR-0352): a command that does not
build, is stopped, or says something inconsistent is no kill, and stops the
run; so is a debug build that fails, is stopped, or names no binary.

Run from the repository root; `just e14-render-merge` records the output.
The source is restored after every mutant, whatever happens.
"""

import json
import os
import pathlib
import signal
import subprocess
import sys

from cargo_test_results import parse as cargo_result

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "spikes/own-renderer/server/src"
MERGE = SRC / "render_merge.rs"
MAIN = SRC / "main.rs"
STORE = SRC / "store.rs"
STORE_PG = SRC / "store_pg.rs"
FEED = SRC / "feed.rs"
FEED_PG = SRC / "feed_pg.rs"
RESOURCE = ROOT / "runtime/pw-resource/src/lib.rs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "a render's kept answer is never given",
        "memory",
        MERGE,
        "            if let Some(answer) = render.answered(&session, &name, args) {\n",
        "            if let Some(answer) = render.answered(&session, &name, args).filter(|_| false) {\n",
    ),
    (
        "a call is answered as one with other arguments was",
        "memory",
        MERGE,
        "            .find(|((s, n, a), _)| s == session && n == name && a.as_slice() == args)\n",
        "            .find(|((s, n, a), _)| s == session && n == name && (a.as_slice() == args || !args.is_empty()))\n",
    ),
    (
        "a call is answered as one for another session was",
        "memory",
        MERGE,
        "            .find(|((s, n, a), _)| s == session && n == name && a.as_slice() == args)\n",
        "            .find(|((s, n, a), _)| (s == session || s != session) && n == name && a.as_slice() == args)\n",
    ),
    (
        "a failed call is kept as its answer",
        "memory",
        MERGE,
        "            let answer = op(args)?;\n",
        "            let answer = match op(args) {\n"
        "                Ok(answer) => answer,\n"
        "                Err(why) => vec![Val::String(why)],\n"
        "            };\n",
    ),
    (
        "a render is kept past its end",
        "unit",
        MERGE,
        "                reading.retain(|r| !Arc::ptr_eq(r, &mine));\n",
        "                reading.retain(|r| !Arc::ptr_eq(r, &mine) || Arc::ptr_eq(r, &mine));\n",
    ),
    (
        "a render begun inside another is its own",
        "memory",
        MERGE,
        "    if READING.with(|reading| reading.borrow().iter().any(|r| r.layer == layer)) {\n",
        "    if READING.with(|reading| reading.borrow().iter().any(|r| r.layer == layer && r.layer != layer)) {\n",
    ),
    (
        "a render answers for another layer",
        "memory",
        MERGE,
        "            .find(|r| r.layer == layer)\n",
        "            .find(|r| r.layer == layer || r.layer != layer)\n",
    ),
    (
        "a render's queries each read as a query of their own",
        "memory",
        MERGE,
        "            Some(snapshot) => data.reads_in(snapshot, session, stopped),\n",
        "            Some(_) => data.reads(session, stopped),\n",
    ),
    (
        "a render takes no snapshot, and no seat",
        "postgres",
        MERGE,
        "        snapshot: data.snapshot(),\n",
        "        snapshot: None,\n",
    ),
    (
        "the drain's count and its documents are two renders",
        "memory",
        MAIN,
        "            Err(why) => {\n"
        "                drop(render);\n"
        "                return self.unshowable_session(session, &why);\n"
        "            }\n"
        "        };\n",
        "            Err(why) => {\n"
        "                drop(render);\n"
        "                return self.unshowable_session(session, &why);\n"
        "            }\n"
        "        };\n"
        "        drop(render);\n"
        "        let render = render_merge::begin(&*self.data, self.queries.epoch());\n",
    ),
    (
        "each document read again is a render of its own",
        "memory",
        MAIN,
        '        let mut read = Vec::new();\n        let mut unread = Vec::new();\n        for (doc, reader) in live {\n',
        '        drop(render);\n        let render = ();\n        let mut read = Vec::new();\n        let mut unread = Vec::new();\n        for (doc, reader) in live {\n            let _own = render_merge::begin(&*self.data, self.queries.epoch());\n',
    ),
    (
        "a page's bindings are not one render",
        "memory",
        MAIN,
        "        let _render = render_merge::begin(&*self.data, self.queries.epoch());\n"
        "        let page = document.map_or_else(\n",
        "        let page = document.map_or_else(\n",
    ),
    (
        "a call answered again is counted as made",
        "memory",
        MAIN,
        '                let f = counted(&name, f);\n                let f = match &render {\n                    Some(render) => render.once(\n                        &self\n                            .identity\n                            .principals()\n                            .receipt_reader(session)\n                            .to_string(),\n                        &name,\n                        f,\n                    ),\n                    None => f,\n                };\n',
        '                let f = match &render {\n                    Some(render) => render.once(\n                        &self\n                            .identity\n                            .principals()\n                            .receipt_reader(session)\n                            .to_string(),\n                        &name,\n                        f,\n                    ),\n                    None => f,\n                };\n                let f = counted(&name, f);\n',
    ),
    (
        "the platform's calls are merged",
        "memory",
        MAIN,
        "        host.extend(platform.into_iter().map(|(name, f)| {\n"
        "            let f = counted(&name, f);\n",
        "        host.extend(platform.into_iter().map(|(name, f)| {\n"
        "            let f = counted(&name, f);\n"
        "            let f = match &render {\n"
        "                Some(render) => render.once(session, &name, f),\n"
        "                None => f,\n"
        "            };\n",
    ),
    (
        "a render reads as any reader does what was invalidated since it began",
        "memory",
        MAIN,
        "                self.queries\n"
        "                    .fetch_cancellable_as_of(manifest, key, render.as_of(), load)\n",
        "                let _ = render;\n"
        "                self.queries.fetch_cancellable(manifest, key, load)\n",
    ),
    (
        "the drain's render is held through its telling",
        "memory",
        MAIN,
        "        self.tell_in(\n"
        "            session,\n"
        "            told,\n"
        "            render,\n",
        "        let _held = render;\n"
        "        self.tell_in(\n"
        "            session,\n"
        "            told,\n"
        "            render_merge::begin(&*self.data, self.queries.epoch()),\n",
    ),
    (
        "a session's documents' render is held through their telling",
        "memory",
        MAIN,
        "        let render = render_merge::begin(&*self.data, self.queries.epoch());\n"
        "        self.tell_in(session, told, render, entry, version, speculated);\n",
        "        let _held = render_merge::begin(&*self.data, self.queries.epoch());\n"
        "        self.tell_in(session, told, render_merge::begin(&*self.data, self.queries.epoch()), entry, version, speculated);\n",
    ),
    (
        "a session's documents' render begins before they are taken",
        "memory",
        MAIN,
        "        let told = self.to_tell(session, which);\n"
        "        let render = render_merge::begin(&*self.data, self.queries.epoch());\n",
        "        let render = render_merge::begin(&*self.data, self.queries.epoch());\n"
        "        let told = self.to_tell(session, which);\n",
    ),
    (
        "the drain's render begins before its documents are taken",
        "memory",
        MAIN,
        "        let told = existing.then(|| self.to_tell(session, |_| true));\n"
        "        let render = render_merge::begin(&*self.data, self.queries.epoch());\n",
        "        let render = render_merge::begin(&*self.data, self.queries.epoch());\n"
        "        let told = existing.then(|| self.to_tell(session, |_| true));\n",
    ),
    (
        "a document that could not be read is said so inside its render",
        "memory",
        MAIN,
        "                Err(why) => unread.push((doc, why)),\n",
        "                Err(why) => {\n"
        "                    self.unshowable(&doc, &why);\n"
        "                    unread.push((doc, why));\n"
        "                }\n",
    ),
    (
        "a session whose count could not be read is said so inside its render",
        "memory",
        MAIN,
        "            Err(why) => {\n"
        "                drop(render);\n"
        "                return self.unshowable_session(session, &why);\n",
        "            Err(why) => {\n"
        "                let _ = &render;\n"
        "                return self.unshowable_session(session, &why);\n",
    ),
    (
        "a render past the bound is seated",
        "postgres",
        FEED_PG,
        "        while seats.taken >= self.bound {\n",
        "        while seats.taken >= self.bound * 100 {\n",
    ),
    # The integrator's answer of 2026-10-17: the seats and the seat wait are a
    # deployment's settings, these defaults where it says nothing.
    (
        "a render past its deployment's seats is seated, up to the default's",
        "memory",
        FEED_PG,
        "        while seats.taken >= self.bound {\n",
        "        while seats.taken >= RENDER_CONNECTIONS {\n",
    ),
    (
        "a render waits a test host's time for its seat, not its deployment's",
        "memory",
        FEED_PG,
        "            wait: Mutex::new(seats.wait),\n",
        "            wait: Mutex::new(SEAT_WAIT),\n",
    ),
    (
        "a deployment's count of seats is not read",
        "memory",
        FEED_PG,
        "                Ok(n) if n >= 1 => n,\n",
        "                Ok(n) if n >= 1 => RENDER_CONNECTIONS,\n",
    ),
    (
        "a deployment's count of no seats is read",
        "memory",
        FEED_PG,
        "                Ok(n) if n >= 1 => n,\n",
        "                Ok(n) => n,\n",
    ),
    (
        "an empty count of seats is refused, not the default",
        "memory",
        FEED_PG,
        "        if let Some(v) = seats.filter(|v| !v.is_empty()) {\n",
        "        if let Some(v) = seats {\n",
    ),
    (
        "a deployment's seat wait is not read",
        "memory",
        FEED_PG,
        "                Ok(n) if n >= 1 => std::time::Duration::from_secs(n),\n",
        "                Ok(n) if n >= 1 => SEAT_WAIT,\n",
    ),
    (
        "a deployment's seat wait of no seconds is read",
        "memory",
        FEED_PG,
        "                Ok(n) if n >= 1 => std::time::Duration::from_secs(n),\n",
        "                Ok(n) => std::time::Duration::from_secs(n),\n",
    ),
    # And the table's and a turn's checks run in a debug build, the
    # development server's as much as a test's.
    (
        "the table's check is a test's alone, not a debug build's",
        "debug",
        MAIN,
        "        #[cfg(debug_assertions)]\n"
        "        assert!(\n"
        "            !render_merge::in_a_render(),\n"
        "            \"the subscriber table taken in a render (NEXT 4c)\"\n",
        "        #[cfg(test)]\n"
        "        assert!(\n"
        "            !render_merge::in_a_render(),\n"
        "            \"the subscriber table taken in a render (NEXT 4c)\"\n",
    ),
    (
        "a turn's check is a test's alone, not a debug build's",
        "debug",
        MAIN,
        "        #[cfg(debug_assertions)]\n"
        "        assert!(\n"
        "            !render_merge::in_a_render(),\n"
        "            \"a session's turn taken in a render (NEXT 4c)\"\n",
        "        #[cfg(test)]\n"
        "        assert!(\n"
        "            !render_merge::in_a_render(),\n"
        "            \"a session's turn taken in a render (NEXT 4c)\"\n",
    ),
    (
        "a seat is never given back",
        "postgres",
        FEED_PG,
        "            seats.taken -= 1;\n"
        "            self.renders.freed.notify_one();\n",
        "            seats.taken -= 0;\n"
        "            self.renders.freed.notify_one();\n",
    ),
    (
        "an idle connection is never taken again",
        "bound",
        FEED_PG,
        "            while let Some(c) = seats.idle.pop() {\n",
        "            while let Some(c) = seats.idle.pop().filter(|_| false) {\n",
    ),
    (
        "a render's transaction ends after every read",
        "postgres",
        FEED_PG,
        "        let out = f(conn.as_mut().expect(\"opened\"));\n"
        "        if out.is_err() {\n",
        "        let out = f(conn.as_mut().expect(\"opened\"));\n"
        "        if out.is_err() || out.is_ok() {\n",
    ),
    (
        "a render keeps its transaction after a read that failed",
        "postgres",
        FEED_PG,
        "            self.renders.ended(conn.take().expect(\"opened\"), \"ROLLBACK\");\n",
        "            let _ = &conn;\n",
    ),
    (
        "a refused seat is read in",
        "postgres",
        FEED_PG,
        "        if let Some(why) = &self.refused {\n"
        "            return Err(why.clone());\n"
        "        }\n",
        "        if let Some(why) = &self.refused {\n"
        "            let _ = why;\n"
        "        }\n",
    ),
    (
        "the store's render on PostgreSQL reads outside its seat",
        "postgres",
        STORE_PG,
        "        let Some(seat) = snapshot.downcast_ref::<Arc<crate::feed_pg::Seat>>() else {\n",
        "        let Some(seat) = snapshot.downcast_ref::<Arc<crate::feed_pg::Seat>>().filter(|_| false) else {\n",
    ),
    (
        "the feed's render on PostgreSQL reads outside its seat",
        "postgres",
        FEED_PG,
        "        let Some(seat) = snapshot.downcast_ref::<Arc<Seat>>() else {\n",
        "        let Some(seat) = snapshot.downcast_ref::<Arc<Seat>>().filter(|_| false) else {\n",
    ),
    (
        "the store's render in memory reads the store as it is now",
        "memory",
        STORE,
        "            Some(rows) => reads(with_mem(rows.clone()), session, &self.faults, stopped),\n",
        "            Some(_) => self.reads(session, stopped),\n",
    ),
    (
        "the feed's render in memory reads the feed as it is now",
        "memory",
        FEED,
        "            Some(state) => reads_of(state.clone(), self.principals()),\n",
        "            Some(_) => self.reads(session, stopped),\n",
    ),
    (
        "an invalidation of a key leaves its resource unstamped",
        "resource",
        RESOURCE,
        "        st.stamp(&key.resource);\n",
        "        let _ = &key.resource;\n",
    ),
    (
        "an invalidation where keys are named leaves its resource unstamped",
        "resource",
        RESOURCE,
        "        st.stamp(resource);\n"
        "        let mut dropped: Vec<Key> = st\n",
        "        let mut dropped: Vec<Key> = st\n",
    ),
    (
        "a resource's whole invalidation leaves it unstamped",
        "resource",
        RESOURCE,
        "        st.stamp(resource);\n"
        "        st.cache.retain(|key, _| key.resource != resource);\n",
        "        st.cache.retain(|key, _| key.resource != resource);\n",
    ),
    (
        "a resource's expiry leaves it unstamped",
        "resource",
        RESOURCE,
        "        st.stamp(resource);\n"
        "        for (key, entry) in st.cache.iter_mut() {\n",
        "        for (key, entry) in st.cache.iter_mut() {\n",
    ),
    (
        "a read as of an epoch is never alone",
        "resource",
        RESOURCE,
        "                    .is_some_and(|&at| at > as_of)\n",
        "                    .is_some_and(|&at| at > as_of && false)\n",
    ),
    (
        "a read as of the epoch of its resource's last invalidation is alone",
        "resource",
        RESOURCE,
        "                    .is_some_and(|&at| at > as_of)\n",
        "                    .is_some_and(|&at| at >= as_of)\n",
    ),
    (
        "a read alone takes what is kept, or a request running",
        "resource",
        RESOURCE,
        "            if alone {\n"
        "                st.trace.push(Trace::RequestAlone { key: key.clone() });\n",
        "            if alone && false {\n"
        "                st.trace.push(Trace::RequestAlone { key: key.clone() });\n",
    ),
    (
        "a read alone is the key's request",
        "resource",
        RESOURCE,
        "            if !alone {\n"
        "                st.in_flight.insert(key.clone(), flight.clone());\n",
        "            if true {\n"
        "                st.in_flight.insert(key.clone(), flight.clone());\n",
    ),
    (
        "a read alone's value is kept",
        "resource",
        RESOURCE,
        "                    if manifest.cacheable && !alone {\n",
        "                    if manifest.cacheable {\n",
    ),
]

TESTS = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--"]
CARGO = {
    "memory": (TESTS + ["render_merge::"], {"PW_STORE_TEST_LAYER": "memory"}),
    "postgres": (
        TESTS + ["render_merge::", "tests::bounded_statements::"],
        {"PW_STORE_TEST_LAYER": "postgres"},
    ),
    "resource": (
        ["cargo", "test", "--quiet", "--locked", "-p", "pw-resource", "--test", "read_as_of"],
        {},
    ),
    # The merge's own tests alone, over a layer of one number: a render kept
    # past its end keeps its read transaction open on PostgreSQL, and a
    # served test's schema, dropped at its end, would wait for it for good.
    "unit": (TESTS + ["render_merge::tests::"], {}),
    # The bound's own test alone: connections never taken again from the
    # idle ones pile up, one for each render, which the whole suite's renders
    # would push past the shared database's 100.
    "bound": (
        TESTS + ["render_merge::on_postgres_the_renders_bound_holds_under_many_renders_at_once"],
        {"PW_STORE_TEST_LAYER": "postgres"},
    ),
    # The development server as `cargo build` makes it: a debug build, and
    # no test's, where the integrator's answer of 2026-10-17 has the table's
    # and a turn's checks run too. Its binary carries each check's message
    # where the check is compiled in (`debug_checks`).
    "debug": (
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server", "--message-format=json"],
        {},
    ),
}

# The lock-order checks' messages (main.rs, `Table::lock` and `Turns::lock`).
CHECKS = [
    "the subscriber table taken in a render (NEXT 4c)",
    "a session's turn taken in a render (NEXT 4c)",
]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run stops: a command stopped is no kill.
BOUND = 1200


def bounded(cmd, env):
    """(output, returncode), or (output, None) when it ran past the bound."""
    p = subprocess.Popen(cmd, cwd=ROOT, env={**os.environ, **env}, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def cargo_tests(suite):
    """(valid, passed, failed) over the suite's tests (`cargo_test_results`),
    or for `debug`, over the checks the debug build carries."""
    cmd, env = CARGO[suite]
    out, code = bounded(cmd, env)
    if suite == "debug":
        return debug_checks(out, code)
    result = cargo_result(code, out)
    if not result.valid:
        return False, 0, 0
    return True, result.passed, result.failed


def debug_checks(out, code):
    """(valid, carried, missing) over `CHECKS`: each check's message in the
    binary the debug build made, which cargo names (`--message-format=json`,
    its `compiler-artifact` message's `executable`). A build that fails, is
    stopped, or names no binary is no result."""
    if code != 0:
        return False, 0, 0
    executable = None
    for line in out.splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if (
            message.get("reason") == "compiler-artifact"
            and message.get("target", {}).get("name") == "pw-dev-server"
            and message.get("executable")
        ):
            executable = message["executable"]
    if executable is None:
        return False, 0, 0
    binary = pathlib.Path(executable).read_bytes()
    carried = sum(1 for check in CHECKS if check.encode() in binary)
    return True, carried, len(CHECKS) - carried


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    watch = mutation_baseline.mutation_bound.start()
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for need in ("PW_STORE_DATABASE_URL", "PW_FEED_DATABASE_URL"):
        if not os.environ.get(need):
            print(f"FAIL: the postgres suite needs {need}, a throwaway database")
            return 1
    for suite in CARGO:
        built, passed, failed = cargo_tests(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed or watch.stopped:
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
            built, passed, failed = cargo_tests(suite)
        finally:
            path.write_text(original)
        if not built or watch.stopped:
            print(f"{what} [{suite}]: REFUSED (invalid or stopped Cargo producer; no kill)", flush=True)
            mutation_baseline.explain()
            return 1
        if failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
