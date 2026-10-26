#!/usr/bin/env python3
"""Mutation controls for NEXT 6c: a payment at the deployment's provider,
settled only by the provider's word (the payments track, steps one to
seven).

Each mutant undoes one piece:
- the signature: HMAC-SHA256 with a long key not hashed first, or with the
  inner pad the outer's; signatures compared only as far as the shorter, or
  by their last byte; a time ahead of the host's taken however far, or the
  tolerance a minute longer; only the first `v1` signature tried;
- the hearing: an event heard before acted on again; an intent whose amount
  changed settling; every outcome taken as a success; a payment settled on
  behalf of no one; an event remembered before the host acts on it, or an
  outcome not settled answered as settled;
- the beginning: a payment begun with no interaction, keyed by its order
  alone, its intent not saying who began it; a provider that cannot be
  reached trapping the command; a deployment that takes no payments
  answered as one whose provider cannot be reached;
- the host: a reader's press of the settling command taken; a session the
  platform opened on a reader's behalf presented by a browser; the
  settling command run as no reader, or keyed by the payment alone;
- the store: an order awaiting its payment not stopping another; the cart
  kept while it awaits; the provider's page not kept, or not read; a
  payment for another order settling this one; any outcome placing it;
- on PostgreSQL: the payment an order awaits never updated, or never read.

Each must fail the server's payments tests (`payments::`, among them
`tests::store_payments::`) or the store's order placed where the deployment
takes none; the PostgreSQL mutants, the store's payments tests with the
store on PostgreSQL (PW_STORE_TEST_LAYER=postgres). They need a database:
`PW_STORE_DATABASE_URL`, a throwaway one, which each test uses in a schema
of its own; without it this refuses to run.

Run from the repository root; `just e14-payments` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PAYMENTS = ROOT / "spikes/own-renderer/server/src/payments.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
STORE = ROOT / "spikes/own-renderer/server/src/store.rs"
LAYER = ROOT / "spikes/own-renderer/server/src/store_pg.rs"
CHECKOUT = ROOT / "spikes/own-renderer/server/src/checkout.rs"

MEMORY, POSTGRES = "memory", "postgres"

# (what is undone, the layer whose tests kill it, file, anchor, replacement)
MUTANTS = [
    (
        "an intent paid is canceled at the provider",
        MEMORY,
        PAYMENTS,
        "            Status::RequiresPaymentMethod | Status::RequiresAction\n        ) {\n",
        "            Status::RequiresPaymentMethod | Status::RequiresAction | Status::Succeeded\n        ) {\n",
    ),
    (
        "another reader's payment is canceled",
        MEMORY,
        PAYMENTS,
        '        if read["metadata"]["owner"].as_str() != Some(reader.as_str()) {\n',
        '        if read["metadata"]["owner"].as_str() != Some(reader.as_str()) && false {\n',
    ),
    (
        "a payment canceled already is refused as past canceling",
        MEMORY,
        PAYMENTS,
        '                    Some("canceled") => answer(Ok(canceled)),\n',
        '                    Some("canceled_") => answer(Ok(canceled)),\n',
    ),
    # "An answer is read to the connection's close" and "the host waits for
    # its provider without end" undid payments.rs's own HTTP, which track
    # outbound-https replaced with the host's one client: its controls are
    # outbound_https_mutations.py's ("an answer stating no length is taken
    # as empty", "each read is bounded, not the answer", "the payments wait
    # for their provider six times longer").
    (
        "a canceled outcome leaves the order awaiting",
        MEMORY,
        STORE,
        '                if awaits && status == "canceled" {\n',
        '                if awaits && status == "canceled" && false {\n',
    ),
    (
        "an order taken back keeps its lines",
        MEMORY,
        STORE,
        "    r.set_cart(owner, &cart)?;\n    if let Some(code)",
        "    let _ = &cart;\n    if let Some(code)",
    ),
    (
        "a line taken back replaces the cart's of its item",
        MEMORY,
        STORE,
        "            Some(held) => held.quantity += line.quantity,\n",
        "            Some(held) => held.quantity = line.quantity,\n",
    ),
    (
        "an order taken back keeps its code's use",
        MEMORY,
        STORE,
        "        r.release_promo(owner, &code)?;\n",
        "        let _ = code;\n",
    ),
    (
        "an order no longer awaiting is taken back",
        MEMORY,
        STORE,
        "                    Some(a) if awaits && !a.payment.is_empty() => {\n",
        "                    Some(a) if !a.payment.is_empty() => {\n",
    ),
    (
        "on PostgreSQL, an order taken back keeps its code's use",
        POSTGRES,
        CHECKOUT,
        '        "DELETE FROM promo_uses WHERE code = $1 AND owner = $2",\n',
        '        "DELETE FROM promo_uses WHERE code = $1 AND owner = $2 AND false",\n',
    ),
    (
        "a key longer than a block is not hashed first",
        MEMORY,
        PAYMENTS,
        "    if key.len() > BLOCK {\n",
        "    if key.len() > BLOCK && false {\n",
    ),
    (
        "the inner pad is the outer's",
        MEMORY,
        PAYMENTS,
        "    inner.update(k.map(|b| b ^ 0x36));\n",
        "    inner.update(k.map(|b| b ^ 0x5c));\n",
    ),
    (
        "signatures are compared only as far as the shorter",
        MEMORY,
        PAYMENTS,
        "    a.len() == b.len() && a.iter()",
        "    a.iter()",
    ),
    (
        "signatures are compared by their last byte",
        MEMORY,
        PAYMENTS,
        ".fold(0u8, |d, (x, y)| d | (x ^ y)) == 0",
        ".fold(0u8, |_, (x, y)| x ^ y) == 0",
    ),
    (
        "a time ahead of the host's is taken however far",
        MEMORY,
        PAYMENTS,
        "    if (now - seconds).abs() > TOLERANCE {\n",
        "    if now - seconds > TOLERANCE {\n",
    ),
    (
        "the tolerance is a minute longer",
        MEMORY,
        PAYMENTS,
        "pub(crate) const TOLERANCE: i64 = 300;\n",
        "pub(crate) const TOLERANCE: i64 = 360;\n",
    ),
    (
        "only the first `v1` signature is tried",
        MEMORY,
        PAYMENTS,
        "        .iter()\n        .any(|s| same(s.as_bytes(), expected.as_bytes()))\n",
        "        .iter()\n        .take(1)\n        .any(|s| same(s.as_bytes(), expected.as_bytes()))\n",
    ),
    (
        "an event heard before is acted on again",
        MEMORY,
        PAYMENTS,
        '        if self.heard.lock().expect("heard").contains(id) {\n',
        '        if self.heard.lock().expect("heard").contains(id) && false {\n',
    ),
    (
        "an intent whose amount changed settles",
        MEMORY,
        PAYMENTS,
        "        if amount != begun {\n",
        "        if amount != begun && false {\n",
    ),
    (
        "every outcome is taken as a success",
        MEMORY,
        PAYMENTS,
        '                status: text(&read["status"]),\n',
        '                status: "succeeded".to_string(),\n',
    ),
    (
        "a payment is settled on behalf of no one",
        MEMORY,
        PAYMENTS,
        '                owner: text(&read["metadata"]["owner"]),\n',
        "                owner: String::new(),\n",
    ),
    (
        "an event is remembered before the host acts on it",
        MEMORY,
        PAYMENTS,
        "            Ok((event, settled)) => match settle(&settled) {\n",
        "            Ok((event, settled)) => match (self.hearing.remember(&event), settle(&settled)).1 {\n",
    ),
    (
        "an outcome not settled is answered as settled",
        MEMORY,
        PAYMENTS,
        '                Err(why) => (500, format!("not settled: {why}")),\n',
        '                Err(why) => (200, format!("not settled: {why}")),\n',
    ),
    (
        "a payment is begun with no interaction",
        MEMORY,
        PAYMENTS,
        "        let Some(interaction) = interaction.as_deref() else {\n",
        '        let Some(interaction) = interaction.as_deref().or(Some("none")) else {\n',
    ),
    (
        "a payment is keyed by its order alone",
        MEMORY,
        PAYMENTS,
        '            &format!("{order}:{interaction}"),\n',
        '            &format!("{order}:{}", interaction.len() * 0),\n',
    ),
    (
        "the intent does not say who began it",
        MEMORY,
        PAYMENTS,
        '        let asked = serde_json::json!({ "order": order, "owner": owner, "amount": amount });\n',
        '        let asked = serde_json::json!({ "order": order, "owner": "", "amount": amount, "_": owner });\n',
    ),
    (
        "a provider that cannot be reached traps the command",
        MEMORY,
        PAYMENTS,
        "            Ok(_) | Err(_) => failed(),\n",
        # The closure still called, so its error type is still inferred.
        '            Ok(_) | Err(_) => failed().and(Err("no provider".to_string())),\n',
    ),
    (
        "a deployment that takes no payments is one whose provider is not there",
        MEMORY,
        PAYMENTS,
        '            "not-taken".into(),\n',
        '            "transport-failed".into(),\n',
    ),
    (
        "a reader's press of the settling command is taken",
        MEMORY,
        SERVER,
        "                server.components.contains_key(id) && !server.invoked_by_platform(id)\n",
        "                server.components.contains_key(id)\n",
    ),
    (
        "a browser may present a session the platform opened on a reader's behalf",
        MEMORY,
        SERVER,
        "        Some(id) if !id.starts_with(identity::ON_BEHALF) => (id.to_string(), false),\n",
        "        Some(id) => (id.to_string(), false),\n",
    ),
    (
        "the settling command runs as no reader",
        MEMORY,
        SERVER,
        '            &format!("payment-{}", settled.intent),\n            &settled.owner,\n',
        '            &format!("payment-{}", settled.intent),\n            "",\n',
    ),
    (
        "the settling command is keyed by the payment alone",
        MEMORY,
        SERVER,
        '        let interaction = format!("settle-{}-{status}", settled.intent);\n',
        '        let interaction = format!("settle-{}", settled.intent);\n',
    ),
    (
        "an order awaiting its payment does not stop another",
        MEMORY,
        STORE,
        # Track `scheduled-orders` reads the reader's order's status once,
        # for this refusal and its own beside a scheduled order.
        "                if current.as_deref() == Some(AWAITING) {\n"
        '                    return Ok(declared("awaiting-payment"));\n',
        "                if current.as_deref() == Some(AWAITING) && false {\n"
        '                    return Ok(declared("awaiting-payment"));\n',
    ),
    (
        "the cart is kept while the order awaits",
        MEMORY,
        STORE,
        "                r.set_status(owner, Some(AWAITING))?;\n                r.set_cart(owner, &[])?;\n",
        "                r.set_status(owner, Some(AWAITING))?;\n",
    ),
    (
        "the provider's page is not kept",
        MEMORY,
        STORE,
        "                            page: page.clone(),\n",
        "                            page: String::new(),\n",
    ),
    (
        "an awaiting order's page is not read",
        MEMORY,
        STORE,
        "    let page = r.awaiting(owner)?.map(|a| a.page).unwrap_or_default();\n",
        "    let page = r.awaiting(owner)?.map(|_| String::new()).unwrap_or_default();\n",
    ),
    (
        "a payment for another order settles this one",
        MEMORY,
        STORE,
        "                        .is_some_and(|a| a.reference == *reference);\n",
        "                        .is_some_and(|_| true);\n",
    ),
    (
        "any outcome places the order",
        MEMORY,
        STORE,
        '                if awaits && status == "succeeded" {\n',
        "                if awaits {\n",
    ),
    (
        "on PostgreSQL, the payment an order awaits is never updated",
        POSTGRES,
        LAYER,
        "ON CONFLICT (owner) DO UPDATE SET reference = $2, payment = $3, page = $4",
        "ON CONFLICT (owner) DO NOTHING",
    ),
    (
        "on PostgreSQL, the payment an order awaits is never read",
        POSTGRES,
        LAYER,
        '"SELECT reference, payment, page FROM order_payments WHERE owner = $1",\n',
        '"SELECT reference, payment, page FROM order_payments WHERE owner = $1 AND false",\n',
    ),
]

CARGO = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--"]

# The tests each layer's mutants must fail.
TESTS = {
    MEMORY: (
        CARGO + ["payments::", "tests::an_order_is_placed_from_the_cart_and_reaches_its_open_page"],
        {"PW_STORE_TEST_LAYER": "memory"},
    ),
    POSTGRES: (
        CARGO + ["tests::store_payments::"],
        {"PW_STORE_TEST_LAYER": "postgres"},
    ),
}

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def run_tests(layer):
    """(built, passed, failed) for one layer's tests."""
    cmd, extra = TESTS[layer]
    env = {**os.environ, **extra}
    p = subprocess.Popen(cmd, cwd=ROOT, env=env, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(a) for a, _ in found), sum(int(b) for _, b in found)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    if not os.environ.get("PW_STORE_DATABASE_URL"):
        print("FAIL: PW_STORE_DATABASE_URL is not set: the PostgreSQL mutants "
              "could not be killed, and the baseline would not hold the layer")
        return 1
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for layer in (MEMORY, POSTGRES):
        built, passed, failed = run_tests(layer)
        print(f"baseline ({layer}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, layer, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = run_tests(layer)
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
