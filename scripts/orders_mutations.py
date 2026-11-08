#!/usr/bin/env python3
"""Mutation controls for ADR-0193: an order is placed from the cart, and its
page follows it as the store moves it along.

Each mutant undoes one piece of the development server's:
- an empty cart placing an order;
- a placed order leaving the cart as it was, and so committing nothing;
- a placed order not recorded;
- the store's change not sent to the session's open pages;
- the store's change sent against the cart's entry, or at a version that
  does not advance;
- the cart's value sent with the store's change.

Every mutant must fail the development server's tests of an order.

Run from the repository root; `just e14-orders` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
STORE_DATA = ROOT / "spikes/own-renderer/server/src/store.rs"
CHECKOUT_RS = ROOT / "spikes/own-renderer/server/src/checkout.rs"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "an empty cart places an order",
        STORE_DATA,
        # Re-anchored by track store-pg: the store's operations, built once
        # over either layer's rows.
        # Re-anchored by track store-accounts: an owner's, a session's or a user's.
        # Re-anchored by ADR-0332: the store places an order through
        # `#await-payment`, and no program reaches `#place` since (train-5's
        # run found these two surviving there).
        "                if lines.is_empty() {\n"
        '                    return Ok(declared("nothing-to-order"));\n',
        "                if false {\n"
        '                    return Ok(declared("nothing-to-order"));\n',
    ),
    (
        "a placed order leaves the cart as it was",
        STORE_DATA,
        # Re-anchored by track store-pg: the store's operations, built once
        # over either layer's rows.
        # Re-anchored by track store-accounts: an owner's, a session's or a user's.
        # Re-anchored by ADR-0332, as the one above.
        "                r.set_status(owner, Some(AWAITING))?;\n                r.set_cart(owner, &[])?;\n",
        "                r.set_status(owner, Some(AWAITING))?;\n",
    ),
    (
        "a placed order is not recorded",
        CHECKOUT_RS,
        # Re-anchored by track store-pg: the store's operations, built once
        # over either layer's rows.
        # Re-anchored by track store-accounts: an owner's, a session's or a user's.
        # Re-anchored by track order-total: a user's order is placed with its
        # charges (`checkout.rs`), which the store's `place_order` calls.
        "    r.place(owner, lines)?;\n",
        "",
    ),
    (
        "the store's change is not sent",
        SERVER,
        # Re-anchored by track store-pg: the store's operations, built once
        # over either layer's rows.
        # Re-anchored by track store-accounts: an owner's, a session's or a user's.
        "                |_| {\n"
        "                    server.session_changed(\n"
        "                        &session,\n"
        "                        server.order_event(),\n"
        "                        &server.order_entry(&session),\n"
        "                    )\n"
        "                },\n",
        "                |_| {},\n",
    ),
    (
        "the store's change is sent against the cart's entry",
        SERVER,
        "        self.send_documents(session, entry, version, false);\n",
        "        self.send_documents(session, &self.cart_entry(session), version, false);\n",
    ),
    (
        "the store's change is sent at a version that does not advance",
        SERVER,
        "        self.clock.advance(1);\n"
        "        let version = Version(self.clock.now());\n"
        "        self.send_documents(session, entry, version, false);\n",
        "        let version = Version(self.clock.now());\n"
        "        self.send_documents(session, entry, version, false);\n",
    ),
    (
        "the cart's value is sent with the store's change",
        SERVER,
        "                .filter(|_| speculated)\n",
        "",
    ),
]

TESTS = [
    "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
    "an_order_is_placed_from_the_cart_and_reaches_its_open_page",
    "an_empty_cart_places_no_order",
    "the_store_moving_an_order_along_reaches_its_open_page",
]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def run_tests():
    """(built, passed, failed) over the tests."""
    p = subprocess.Popen(TESTS, cwd=ROOT, start_new_session=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(n) for n, _ in found), sum(int(f) for _, f in found)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built, passed, failed = run_tests()
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
            built, passed, failed = run_tests()
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

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
