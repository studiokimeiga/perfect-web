//! **A deployment's payments** (NEXT item 6, step 6c): what a payment
//! provider tells the host, held to its signature.
//!
//! A provider tells the host a payment's outcome by a webhook: a request
//! whose body is the event, and whose `Stripe-Signature` header is
//! `t=<seconds>,v1=<hex>`, the HMAC-SHA256 of `"<seconds>.<body>"` under the
//! endpoint's secret (Stripe's scheme, docs.stripe.com "Webhooks", read
//! 2026-10-12). The host acts on an event only once it is shown to be the
//! provider's: signed with the secret, within five minutes of the host's
//! clock, compared in constant time. Fulfilment is never the browser's word.
//!
//! HMAC is written here over `sha2`, which the workspace has, rather than
//! taken as a new crate: it is RFC 2104's two hashes, and RFC 4231's test
//! vectors hold it.

use sha2::{Digest, Sha256};

/// SHA-256's block, in bytes (RFC 2104's `B`).
const BLOCK: usize = 64;

/// **HMAC-SHA256 of `message` under `key`** (RFC 2104): `H((K ^ opad) ||
/// H((K ^ ipad) || message))`, a key longer than a block hashed first.
pub(crate) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether two byte strings are equal, in time that depends on their
/// lengths alone: a signature is never compared byte by byte until the first
/// difference, which would tell an attacker how much of a guess was right.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// **A webhook's `Stripe-Signature` header**, as a provider writes it for
/// `body` at `seconds` under `secret`.
pub(crate) fn signature(secret: &[u8], seconds: i64, body: &[u8]) -> String {
    let mut signed = format!("{seconds}.").into_bytes();
    signed.extend_from_slice(body);
    format!("t={seconds},v1={}", hex(&hmac_sha256(secret, &signed)))
}

/// How far a webhook's time may be from the host's, in seconds: five
/// minutes, Stripe's libraries' default.
pub(crate) const TOLERANCE: i64 = 300;

/// **Why a webhook is not the provider's word.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refused {
    /// No header, or one without a time or a `v1` signature.
    Unsigned,
    /// Signed at a time too far from the host's.
    Stale { seconds: i64, now: i64 },
    /// No `v1` signature is the body's under the secret.
    Forged,
}

/// **Whether `header` signs `body` under `secret`**, at `now` (seconds), as
/// a provider signs it. Every `v1` signature the header carries is tried, as
/// a provider rolling its secret sends two; any other scheme is ignored.
pub(crate) fn verify(
    header: Option<&str>,
    body: &[u8],
    secret: &[u8],
    now: i64,
) -> Result<(), Refused> {
    let header = header.ok_or(Refused::Unsigned)?;
    let mut seconds = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", t)) => seconds = t.parse::<i64>().ok(),
            Some(("v1", s)) => signatures.push(s.to_string()),
            _ => {}
        }
    }
    let seconds = seconds.ok_or(Refused::Unsigned)?;
    if signatures.is_empty() {
        return Err(Refused::Unsigned);
    }
    if (now - seconds).abs() > TOLERANCE {
        return Err(Refused::Stale { seconds, now });
    }
    let mut signed = format!("{seconds}.").into_bytes();
    signed.extend_from_slice(body);
    let expected = hex(&hmac_sha256(secret, &signed));
    if signatures
        .iter()
        .any(|s| same(s.as_bytes(), expected.as_bytes()))
    {
        Ok(())
    } else {
        Err(Refused::Forged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    /// **RFC 4231's test cases for HMAC-SHA256**: a short key, a key shorter
    /// than the output, a key of 0xaa bytes, and a key longer than a block.
    #[test]
    fn hmac_sha256_is_rfc_4231s() {
        for (key, data, want) in [
            (
                unhex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"),
                b"Hi There".to_vec(),
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                b"Jefe".to_vec(),
                b"what do ya want for nothing?".to_vec(),
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                vec![0xaa; 20],
                vec![0xdd; 50],
                "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
            ),
            (
                vec![0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First".to_vec(),
                "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            ),
        ] {
            assert_eq!(hex(&hmac_sha256(&key, &data)), want);
        }
    }

    /// **A webhook signed with the secret, now, is the provider's**; the same
    /// body under another secret, changed, too old or unsigned is not.
    #[test]
    fn a_webhook_is_the_providers_only_as_it_signed_it() {
        let secret = b"whsec_development";
        let body = br#"{"id":"evt_1","type":"payment_intent.succeeded"}"#;
        let header = signature(secret, 1_000_000, body);
        assert_eq!(verify(Some(&header), body, secret, 1_000_000), Ok(()));
        // Within five minutes either way.
        assert_eq!(verify(Some(&header), body, secret, 1_000_300), Ok(()));
        assert_eq!(verify(Some(&header), body, secret, 999_700), Ok(()));
        assert_eq!(
            verify(Some(&header), body, secret, 1_000_301),
            Err(Refused::Stale {
                seconds: 1_000_000,
                now: 1_000_301
            })
        );
        // And a time ahead of the host's past it, as far.
        assert_eq!(
            verify(Some(&header), body, secret, 999_699),
            Err(Refused::Stale {
                seconds: 1_000_000,
                now: 999_699
            })
        );
        assert_eq!(
            verify(Some(&header), body, b"whsec_other", 1_000_000),
            Err(Refused::Forged)
        );
        let changed = br#"{"id":"evt_1","type":"payment_intent.canceled"}"#;
        assert_eq!(
            verify(Some(&header), changed, secret, 1_000_000),
            Err(Refused::Forged)
        );
        assert_eq!(
            verify(None, body, secret, 1_000_000),
            Err(Refused::Unsigned)
        );
        assert_eq!(
            verify(Some("t=1000000"), body, secret, 1_000_000),
            Err(Refused::Unsigned)
        );
        // A second `v1`, as a provider rolling its secret sends, and an
        // unknown scheme beside it, ignored.
        let rolled = format!("{header},v0=abc,v1={}", "0".repeat(64));
        assert_eq!(verify(Some(&rolled), body, secret, 1_000_000), Ok(()));
        let other_first = format!("t=1000000,v1={},{}", "0".repeat(64), &header[10..]);
        assert_eq!(verify(Some(&other_first), body, secret, 1_000_000), Ok(()));
    }

    /// **Equal lengths compared whole**: a difference anywhere is a
    /// difference, and lengths that differ are never equal.
    #[test]
    fn signatures_are_compared_whole() {
        assert!(same(b"abcd", b"abcd"));
        assert!(!same(b"abcd", b"abce"));
        assert!(!same(b"abcd", b"xbcd"));
        assert!(!same(b"abcd", b"abc"));
        assert!(same(b"", b""));
    }
}

/// **Where a payment is** (Stripe's PaymentIntent lifecycle, docs.stripe.com
/// "How PaymentIntents work", read 2026-10-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// Waiting for a card, and where a declined card leaves it, to try
    /// another.
    RequiresPaymentMethod,
    /// The card's bank asks the reader to authenticate (3-D Secure).
    RequiresAction,
    Processing,
    Succeeded,
    Canceled,
}

impl Status {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Status::RequiresPaymentMethod => "requires_payment_method",
            Status::RequiresAction => "requires_action",
            Status::Processing => "processing",
            Status::Succeeded => "succeeded",
            Status::Canceled => "canceled",
        }
    }
}

/// **A payment for one order**, as the provider holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Intent {
    pub(crate) id: String,
    /// What the browser's hosted fields are opened with, and nothing else
    /// may do with it: it names the intent to the provider's own page.
    pub(crate) client_secret: String,
    pub(crate) order: String,
    /// Who began it: the reader whose command did, as the host names its
    /// principal. The host settles the payment on their behalf (step seven).
    pub(crate) owner: String,
    /// In cents: an amount's minor units (ADR-0327).
    pub(crate) amount: i64,
    /// The amount it was begun with. An intent's amount can be changed at a
    /// provider before it is paid; the host holds a payment to what it began.
    pub(crate) begun: i64,
    pub(crate) status: Status,
    /// Why the last card was declined, where one was.
    pub(crate) declined: Option<&'static str>,
}

/// **What the provider tells the host**, each an event it signs and sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Event {
    pub(crate) id: String,
    /// `payment_intent.succeeded`, `payment_intent.payment_failed` or
    /// `payment_intent.requires_action`.
    pub(crate) kind: &'static str,
    pub(crate) intent: String,
}

/// **How a card the hosted fields were given answers**, by Stripe's test
/// numbers (docs.stripe.com "Testing", read 2026-10-12). Any other number is
/// declined: the development provider charges no real card.
fn outcome(card: &str) -> Result<Status, &'static str> {
    let digits: String = card.chars().filter(char::is_ascii_digit).collect();
    match digits.as_str() {
        "4242424242424242" => Ok(Status::Succeeded),
        "4000000000003220" | "4000002760003184" => Ok(Status::RequiresAction),
        "4000000000009995" => Err("insufficient_funds"),
        _ => Err("card_declined"),
    }
}

/// How long an idempotency key answers, in seconds: a day, as Stripe keeps
/// one at least (docs.stripe.com "Idempotent requests", read 2026-10-12).
pub(crate) const KEY_LIFE: i64 = 24 * 60 * 60;

/// **Why the provider refused a request.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderError {
    /// A key sent again with other parameters: Stripe refuses it, and so
    /// does this.
    KeyReused,
    UnknownIntent,
    /// A card given to an intent that takes none now.
    NotPayable(Status),
    /// An intent past canceling (step eight): paid, or canceled already.
    NotCancelable(Status),
}

/// **The development payment provider** (NEXT 6c): what a provider such as
/// Stripe does, for a host to be built and tested against, charging nothing.
/// Its intents are idempotent by key; its hosted card fields answer by
/// Stripe's test numbers; each change is an event the host is told of.
#[derive(Debug, Default)]
pub(crate) struct DevProvider {
    intents: std::collections::BTreeMap<String, Intent>,
    /// An idempotency key: what it was first sent with, the intent it made,
    /// and when.
    keys: std::collections::BTreeMap<String, (String, String, i64)>,
    /// Events not yet delivered, in the order they happened.
    pub(crate) outbox: Vec<Event>,
    next: u64,
}

impl DevProvider {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}_dev{:06}", self.next)
    }

    /// **An intent's amount changed at the provider** (a test's switch), as
    /// Stripe lets an unpaid intent's amount be updated.
    pub(crate) fn amend(&mut self, id: &str, amount: i64) -> Result<(), ProviderError> {
        let intent = self
            .intents
            .get_mut(id)
            .ok_or(ProviderError::UnknownIntent)?;
        intent.amount = amount;
        Ok(())
    }

    /// **An intent for `order`'s `amount`, by `key`** (Stripe's
    /// `Idempotency-Key`): the key sent again within a day with the same
    /// order and amount answers the intent it made, and with others is
    /// refused; a key older than a day makes a new one.
    pub(crate) fn create(
        &mut self,
        key: &str,
        order: &str,
        owner: &str,
        amount: i64,
        now: i64,
    ) -> Result<Intent, ProviderError> {
        let sent = format!("{order}\u{1f}{owner}\u{1f}{amount}");
        if let Some((first, intent, at)) = self.keys.get(key)
            && now - at < KEY_LIFE
        {
            if *first != sent {
                return Err(ProviderError::KeyReused);
            }
            return self
                .intents
                .get(intent)
                .cloned()
                .ok_or(ProviderError::UnknownIntent);
        }
        let id = self.id("pi");
        let intent = Intent {
            client_secret: format!("{id}_secret_{:06}", self.next * 7919 % 1_000_000),
            id: id.clone(),
            order: order.to_string(),
            owner: owner.to_string(),
            amount,
            begun: amount,
            status: Status::RequiresPaymentMethod,
            declined: None,
        };
        self.intents.insert(id.clone(), intent.clone());
        self.keys.insert(key.to_string(), (sent, id, now));
        Ok(intent)
    }

    pub(crate) fn read(&self, id: &str) -> Result<Intent, ProviderError> {
        self.intents
            .get(id)
            .cloned()
            .ok_or(ProviderError::UnknownIntent)
    }

    fn by_secret(&mut self, client_secret: &str) -> Result<&mut Intent, ProviderError> {
        self.intents
            .values_mut()
            .find(|i| i.client_secret == client_secret)
            .ok_or(ProviderError::UnknownIntent)
    }

    fn emit(&mut self, kind: &'static str, intent: String) {
        let id = self.id("evt");
        self.outbox.push(Event { id, kind, intent });
    }

    /// **A card given to the hosted fields of the intent `client_secret`
    /// names**: it succeeds, asks the reader to authenticate, or is declined
    /// and the intent waits for another card.
    pub(crate) fn pay(&mut self, client_secret: &str, card: &str) -> Result<Intent, ProviderError> {
        let intent = self.by_secret(client_secret)?;
        if intent.status != Status::RequiresPaymentMethod {
            return Err(ProviderError::NotPayable(intent.status));
        }
        let (kind, id) = match outcome(card) {
            Ok(status) => {
                intent.status = status;
                intent.declined = None;
                let kind = match status {
                    Status::Succeeded => "payment_intent.succeeded",
                    _ => "payment_intent.requires_action",
                };
                (kind, intent.id.clone())
            }
            Err(why) => {
                intent.declined = Some(why);
                ("payment_intent.payment_failed", intent.id.clone())
            }
        };
        self.emit(kind, id.clone());
        self.read(&id)
    }

    /// **The reader's answer to the bank's challenge**: authenticated, the
    /// payment succeeds; not, it is declined and waits for another card.
    pub(crate) fn authenticate(
        &mut self,
        client_secret: &str,
        passed: bool,
    ) -> Result<Intent, ProviderError> {
        let intent = self.by_secret(client_secret)?;
        if intent.status != Status::RequiresAction {
            return Err(ProviderError::NotPayable(intent.status));
        }
        let id = intent.id.clone();
        if passed {
            intent.status = Status::Succeeded;
            self.emit("payment_intent.succeeded", id.clone());
        } else {
            intent.status = Status::RequiresPaymentMethod;
            intent.declined = Some("authentication_failed");
            self.emit("payment_intent.payment_failed", id.clone());
        }
        self.read(&id)
    }

    /// **An intent canceled** (step eight), as Stripe cancels one: while it
    /// waits for a card or for the reader's bank, and not once it is paid or
    /// canceled (docs.stripe.com "Cancel a PaymentIntent", read 2026-10-13:
    /// it "Returns an error if the PaymentIntent is already canceled or isn't
    /// in a cancelable state").
    pub(crate) fn cancel(&mut self, id: &str) -> Result<Intent, ProviderError> {
        let intent = self
            .intents
            .get_mut(id)
            .ok_or(ProviderError::UnknownIntent)?;
        if !matches!(
            intent.status,
            Status::RequiresPaymentMethod | Status::RequiresAction
        ) {
            return Err(ProviderError::NotCancelable(intent.status));
        }
        intent.status = Status::Canceled;
        self.emit("payment_intent.canceled", id.to_string());
        self.read(id)
    }

    /// **An event as the provider sends it**: its body, and the signature
    /// header for it at `now` under `secret`.
    pub(crate) fn sent(event: &Event, secret: &[u8], now: i64) -> (String, String) {
        let body = serde_json::json!({
            "id": event.id,
            "type": event.kind,
            "data": { "object": { "id": event.intent } },
        })
        .to_string();
        let header = signature(secret, now, body.as_bytes());
        (body, header)
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;

    /// **An intent is canceled until it is paid** (step eight): while it
    /// waits for a card or for the reader's bank, with an event; not once it
    /// is paid, nor again.
    #[test]
    fn an_intent_is_canceled_until_it_is_paid() {
        let mut p = DevProvider::default();
        let waiting = p.create("k1", "o-1", "reader-a", 1250, 0).expect("made");
        let canceled = p.cancel(&waiting.id).expect("canceled");
        assert_eq!(canceled.status, Status::Canceled);
        assert_eq!(
            p.outbox.last().map(|e| e.kind),
            Some("payment_intent.canceled")
        );
        assert_eq!(
            p.cancel(&waiting.id),
            Err(ProviderError::NotCancelable(Status::Canceled))
        );
        let asked = p.create("k2", "o-2", "reader-a", 1250, 0).expect("made");
        p.pay(&asked.client_secret, "4000000000003220")
            .expect("asked to authenticate");
        assert_eq!(p.cancel(&asked.id).map(|i| i.status), Ok(Status::Canceled));
        let paid = p.create("k3", "o-3", "reader-a", 1250, 0).expect("made");
        p.pay(&paid.client_secret, "4242424242424242")
            .expect("paid");
        assert_eq!(
            p.cancel(&paid.id),
            Err(ProviderError::NotCancelable(Status::Succeeded))
        );
    }

    /// **One intent per key**: sent again within a day with the same order
    /// and amount, the same intent; with another amount, refused; after a
    /// day, a new one.
    #[test]
    fn an_intent_is_made_once_per_key() {
        let mut p = DevProvider::default();
        let first = p
            .create("order-1:press-a", "order-1", "reader-a", 1250, 0)
            .expect("made");
        assert_eq!(first.status, Status::RequiresPaymentMethod);
        let again = p
            .create("order-1:press-a", "order-1", "reader-a", 1250, 60)
            .expect("answered");
        assert_eq!(again, first);
        assert_eq!(
            p.create("order-1:press-a", "order-1", "reader-a", 1300, 60),
            Err(ProviderError::KeyReused)
        );
        let later = p
            .create("order-1:press-a", "order-1", "reader-a", 1250, KEY_LIFE)
            .expect("made again");
        assert_ne!(later.id, first.id);
        // Another key is another intent.
        let other = p
            .create("order-2:press-a", "order-2", "reader-a", 1250, 0)
            .expect("made");
        assert_ne!(other.id, first.id);
    }

    /// **A card's outcome by Stripe's test numbers**: 4242 succeeds; 3220
    /// asks to authenticate, and the answer decides; 0002 is declined and
    /// 9995 has insufficient funds, each leaving the intent for another card;
    /// a paid intent takes no card.
    #[test]
    fn a_cards_outcome_is_its_test_numbers() {
        let mut p = DevProvider::default();
        let i = p
            .create("k1", "order-1", "reader-a", 1250, 0)
            .expect("made");
        let declined = p
            .pay(&i.client_secret, "4000 0000 0000 0002")
            .expect("answered");
        assert_eq!(
            (declined.status, declined.declined),
            (Status::RequiresPaymentMethod, Some("card_declined"))
        );
        let poor = p
            .pay(&i.client_secret, "4000000000009995")
            .expect("answered");
        assert_eq!(poor.declined, Some("insufficient_funds"));
        let paid = p
            .pay(&i.client_secret, "4242 4242 4242 4242")
            .expect("answered");
        assert_eq!((paid.status, paid.declined), (Status::Succeeded, None));
        assert_eq!(
            p.pay(&i.client_secret, "4242424242424242"),
            Err(ProviderError::NotPayable(Status::Succeeded))
        );
        let kinds: Vec<&str> = p.outbox.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                "payment_intent.payment_failed",
                "payment_intent.payment_failed",
                "payment_intent.succeeded"
            ]
        );

        let j = p.create("k2", "order-2", "reader-a", 990, 0).expect("made");
        let asked = p
            .pay(&j.client_secret, "4000000000003220")
            .expect("answered");
        assert_eq!(asked.status, Status::RequiresAction);
        let failed = p.authenticate(&j.client_secret, false).expect("answered");
        assert_eq!(
            (failed.status, failed.declined),
            (Status::RequiresPaymentMethod, Some("authentication_failed"))
        );
        p.pay(&j.client_secret, "4000000000003220")
            .expect("asked again");
        let passed = p.authenticate(&j.client_secret, true).expect("answered");
        assert_eq!(passed.status, Status::Succeeded);
        assert_eq!(
            p.pay("pi_nothing_secret_0", "4242424242424242"),
            Err(ProviderError::UnknownIntent)
        );
    }

    /// **What the provider sends is what the host verifies.**
    #[test]
    fn an_event_sent_is_signed_for_the_host() {
        let mut p = DevProvider::default();
        let i = p.create("k", "order-1", "reader-a", 1250, 0).expect("made");
        p.pay(&i.client_secret, "4242424242424242").expect("paid");
        let (body, header) = DevProvider::sent(&p.outbox[0], b"whsec_dev", 1_000);
        assert_eq!(
            verify(Some(&header), body.as_bytes(), b"whsec_dev", 1_000),
            Ok(())
        );
        let json: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(json["type"], "payment_intent.succeeded");
        assert_eq!(json["data"]["object"]["id"], i.id.as_str());
    }
}

// --- the development provider, on its own origin ---------------------------

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A request as the provider reads one: bounded, as a card number and a
/// few fields are all it takes.
struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// A form field of the body (`application/x-www-form-urlencoded`).
    fn field(&self, name: &str) -> Option<String> {
        String::from_utf8_lossy(&self.body)
            .split('&')
            .find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                (k == name).then(|| v.replace('+', " "))
            })
    }
}

const MAX_BODY: usize = 16 * 1024;

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).ok()? == 0 || h == "\r\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return None;
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

fn write(stream: &mut TcpStream, code: u16, mime: &str, extra: &str, body: &[u8]) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\ncontent-type: {mime}\r\ncontent-length: {}\r\n{extra}cache-control: no-store\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// `1250` as `$12.50`, for the hosted fields' button.
fn dollars(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let c = cents.unsigned_abs();
    format!("{sign}${}.{:02}", c / 100, c % 100)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn intent_json(i: &Intent) -> String {
    serde_json::json!({
        "id": i.id,
        "client_secret": i.client_secret,
        "status": i.status.name(),
        "amount": i.amount,
        "currency": "usd",
        // As Stripe's metadata, strings: the order, who began it, and the
        // amount it was begun with.
        "metadata": { "order": i.order, "owner": i.owner, "amount": i.begun.to_string() },
        "last_payment_error": i.declined.map(|code| serde_json::json!({ "code": code })),
    })
    .to_string()
}

/// **The development provider, served on its own origin** (a port of its
/// own): the API a host calls, the hosted card fields a page embeds, and the
/// webhooks it sends the host, each signed.
pub(crate) struct Origin {
    provider: Mutex<DevProvider>,
    secret: Vec<u8>,
    /// Where the host hears the provider: its `/payments/webhook`.
    webhook: String,
    /// The host's origin, the one page that may embed the hosted fields.
    host: String,
    now: fn() -> i64,
    /// A test's: each event sent twice, as a provider may.
    pub(crate) twice: AtomicBool,
    /// A test's: events held, and sent in reverse when let go.
    held: Mutex<Option<Vec<Event>>>,
}

/// The wall clock in seconds since 1970.
pub(crate) fn wall_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Origin {
    pub(crate) fn new(secret: &[u8], webhook: &str, host: &str, now: fn() -> i64) -> Origin {
        Origin {
            provider: Mutex::new(DevProvider::default()),
            secret: secret.to_vec(),
            webhook: webhook.to_string(),
            host: host.to_string(),
            now,
            twice: AtomicBool::new(false),
            held: Mutex::new(None),
        }
    }

    /// A test's: events held from now until [`Origin::release_reversed`].
    pub(crate) fn hold(&self) {
        *self.held.lock().expect("held") = Some(Vec::new());
    }

    /// A test's: the held events sent, the last first.
    pub(crate) fn release_reversed(&self) {
        let held = self.held.lock().expect("held").take().unwrap_or_default();
        for e in held.iter().rev() {
            self.send(e);
        }
    }

    fn send(&self, e: &Event) {
        let (body, header) = DevProvider::sent(e, &self.secret, (self.now)());
        let times = if self.twice.load(Ordering::SeqCst) {
            2
        } else {
            1
        };
        for _ in 0..times {
            if let Err(why) = post(&self.webhook, &body, &header) {
                eprintln!(
                    "development payments: the webhook to {}: {why}",
                    self.webhook
                );
            }
        }
    }

    /// Each event the provider has not yet told the host, told, in order;
    /// or held, where a test holds them.
    fn deliver(&self) {
        let events = std::mem::take(&mut self.provider.lock().expect("provider").outbox);
        let mut held = self.held.lock().expect("held");
        if let Some(h) = held.as_mut() {
            h.extend(events);
            return;
        }
        drop(held);
        for e in &events {
            self.send(e);
        }
    }

    fn page(&self, intent: &Intent, said: &str) -> String {
        let secret = escape(&intent.client_secret);
        let form = match intent.status {
            Status::RequiresPaymentMethod => format!(
                "<form method=\"post\" action=\"/pay/{secret}\"><label>Card number \
                 <input name=\"card\" inputmode=\"numeric\" autocomplete=\"cc-number\" required></label> \
                 <button>Pay {}</button></form>",
                dollars(intent.amount)
            ),
            Status::RequiresAction => format!(
                "<p>Your bank asks you to confirm this payment.</p>\
                 <form method=\"post\" action=\"/pay/{secret}/authenticate\">\
                 <button name=\"passed\" value=\"yes\">Confirm</button> \
                 <button name=\"passed\" value=\"no\">Decline</button></form>"
            ),
            Status::Succeeded => "<p>Paid.</p>".to_string(),
            Status::Processing => "<p>Processing.</p>".to_string(),
            Status::Canceled => "<p>This payment was canceled.</p>".to_string(),
        };
        format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
             <title>Card details</title></head><body><main>{form}\
             <p role=\"status\">{}</p></main></body></html>",
            escape(said)
        )
    }

    fn answer(&self, mut stream: TcpStream) {
        let Some(req) = read_request(&stream) else {
            return;
        };
        let path = req.path.split('?').next().unwrap_or("/").to_string();
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        // The hosted fields: embedded by the host's page alone, and running
        // no script at all.
        let framed = format!(
            "content-security-policy: default-src 'none'; form-action 'self'; frame-ancestors {}\r\n",
            self.host
        );
        let html = "text/html; charset=utf-8";
        let json = "application/json";
        match (req.method.as_str(), segments.as_slice()) {
            ("POST", ["v1", "payment_intents"]) => {
                let Some(key) = req.header("idempotency-key").map(str::to_string) else {
                    return write(&mut stream, 400, json, "", br#"{"error":{"type":"invalid_request_error","message":"an Idempotency-Key is required"}}"#);
                };
                let asked: serde_json::Value =
                    serde_json::from_slice(&req.body).unwrap_or_default();
                let (Some(order), Some(owner), Some(amount)) = (
                    asked["order"].as_str(),
                    asked["owner"].as_str(),
                    asked["amount"].as_i64(),
                ) else {
                    return write(&mut stream, 400, json, "", br#"{"error":{"type":"invalid_request_error","message":"an order, an owner and an amount are required"}}"#);
                };
                let made = self.provider.lock().expect("provider").create(
                    &key,
                    order,
                    owner,
                    amount,
                    (self.now)(),
                );
                match made {
                    Ok(i) => write(&mut stream, 200, json, "", intent_json(&i).as_bytes()),
                    Err(_) => write(&mut stream, 400, json, "", br#"{"error":{"type":"idempotency_error","message":"the key was sent with other parameters"}}"#),
                }
            }
            ("GET", ["v1", "payment_intents", id]) => {
                match self.provider.lock().expect("provider").read(id) {
                    Ok(i) => write(&mut stream, 200, json, "", intent_json(&i).as_bytes()),
                    Err(_) => write(
                        &mut stream,
                        404,
                        json,
                        "",
                        br#"{"error":{"type":"invalid_request_error","message":"no such intent"}}"#,
                    ),
                }
            }
            // Step eight: an intent canceled, as Stripe's API cancels one, its
            // event sent once the answer is written, as a provider's webhook
            // comes after the request that made it.
            ("POST", ["v1", "payment_intents", id, "cancel"]) => {
                let canceled = self.provider.lock().expect("provider").cancel(id);
                match canceled {
                    Ok(i) => {
                        write(&mut stream, 200, json, "", intent_json(&i).as_bytes());
                        self.deliver();
                    }
                    Err(ProviderError::NotCancelable(_)) => {
                        let read = self.provider.lock().expect("provider").read(id);
                        let intent: serde_json::Value = read
                            .map(|i| serde_json::from_str(&intent_json(&i)).unwrap_or_default())
                            .unwrap_or_default();
                        let refused = serde_json::json!({ "error": {
                            "type": "invalid_request_error",
                            "code": "payment_intent_unexpected_state",
                            "payment_intent": intent,
                        } });
                        write(&mut stream, 400, json, "", refused.to_string().as_bytes());
                    }
                    Err(_) => write(
                        &mut stream,
                        404,
                        json,
                        "",
                        br#"{"error":{"type":"invalid_request_error","message":"no such intent"}}"#,
                    ),
                }
            }
            ("GET", ["pay", secret]) => {
                let found = self
                    .provider
                    .lock()
                    .expect("provider")
                    .intents
                    .values()
                    .find(|i| i.client_secret == *secret)
                    .cloned();
                match found {
                    Some(i) => write(
                        &mut stream,
                        200,
                        html,
                        &framed,
                        self.page(&i, "").as_bytes(),
                    ),
                    None => write(
                        &mut stream,
                        404,
                        html,
                        &framed,
                        b"<!doctype html><title>Not found</title>",
                    ),
                }
            }
            ("POST", ["pay", secret]) => {
                let card = req.field("card").unwrap_or_default();
                let paid = self.provider.lock().expect("provider").pay(secret, &card);
                match paid {
                    Ok(i) => {
                        let said = match i.declined {
                            Some("insufficient_funds") => "Your card has insufficient funds.",
                            Some(_) => "Your card was declined.",
                            None => "",
                        };
                        write(
                            &mut stream,
                            200,
                            html,
                            &framed,
                            self.page(&i, said).as_bytes(),
                        );
                        self.deliver();
                    }
                    Err(_) => write(
                        &mut stream,
                        409,
                        html,
                        &framed,
                        b"<!doctype html><title>Not payable</title>",
                    ),
                }
            }
            ("POST", ["pay", secret, "authenticate"]) => {
                let passed = req.field("passed").as_deref() == Some("yes");
                let answered = self
                    .provider
                    .lock()
                    .expect("provider")
                    .authenticate(secret, passed);
                match answered {
                    Ok(i) => {
                        let said = if passed {
                            ""
                        } else {
                            "The payment was not confirmed."
                        };
                        write(
                            &mut stream,
                            200,
                            html,
                            &framed,
                            self.page(&i, said).as_bytes(),
                        );
                        self.deliver();
                    }
                    Err(_) => write(
                        &mut stream,
                        409,
                        html,
                        &framed,
                        b"<!doctype html><title>Not payable</title>",
                    ),
                }
            }
            _ => write(&mut stream, 404, "text/plain", "", b"not found"),
        }
    }
}

/// **The provider's origin, listening**: a thread per connection.
pub(crate) fn serve(listener: TcpListener, origin: Arc<Origin>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let origin = origin.clone();
        std::thread::spawn(move || origin.answer(stream));
    }
}

/// **A POST of `body` to `url`** (`http://host:port/path`), with the
/// provider's signature: the webhook, over a connection of its own.
pub(crate) fn post(url: &str, body: &str, signature: &str) -> Result<u16, String> {
    let rest = url.strip_prefix("http://").ok_or("not an http URL")?;
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_string()), |(a, p)| (a, format!("/{p}")));
    let mut stream = TcpStream::connect(authority).map_err(|e| e.to_string())?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nhost: {authority}\r\ncontent-type: application/json\r\nstripe-signature: {signature}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    stream
        .write_all(body.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut answer = String::new();
    BufReader::new(stream)
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    answer
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| format!("no status: {answer:?}"))
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    /// Each request a test's webhook endpoint heard: its signature header,
    /// and its body.
    type Heard = Arc<Mutex<Vec<(String, String)>>>;

    /// A host's webhook endpoint, for a test: it answers 200 and keeps each
    /// request's signature header and body.
    fn webhook_listener() -> (String, Heard) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!(
            "http://{}/payments/webhook",
            listener.local_addr().expect("addr")
        );
        let heard = Arc::new(Mutex::new(Vec::new()));
        let kept = heard.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                if let Some(req) = read_request(&stream) {
                    let sig = req.header("stripe-signature").unwrap_or("").to_string();
                    kept.lock()
                        .expect("heard")
                        .push((sig, String::from_utf8_lossy(&req.body).into_owned()));
                }
                write(&mut stream, 200, "text/plain", "", b"ok");
            }
        });
        (url, heard)
    }

    fn started(webhook: &str) -> (String, Arc<Origin>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let origin = Arc::new(Origin::new(
            b"whsec_test",
            webhook,
            "http://127.0.0.1:3301",
            || 1_000_000,
        ));
        let served = origin.clone();
        std::thread::spawn(move || serve(listener, served));
        (base, origin)
    }

    /// A request to the provider, answered: its status and body.
    fn ask(
        base: &str,
        method: &str,
        path: &str,
        headers: &str,
        body: &str,
    ) -> (u16, String, String) {
        let authority = base.strip_prefix("http://").expect("http");
        let mut stream = TcpStream::connect(authority).expect("connect");
        let head = format!(
            "{method} {path} HTTP/1.1\r\nhost: {authority}\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).expect("write");
        stream.write_all(body.as_bytes()).expect("write");
        let mut all = String::new();
        stream.read_to_string(&mut all).expect("read");
        let (head, body) = all.split_once("\r\n\r\n").expect("a head");
        let code = head
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .expect("status");
        (code, head.to_string(), body.to_string())
    }

    /// **The provider's API, its hosted fields and its webhook, over the
    /// wire**: an intent made once per key, a card paid in the hosted fields,
    /// and the host told, signed.
    #[test]
    fn a_card_paid_in_the_hosted_fields_is_told_to_the_host_signed() {
        let (webhook, heard) = webhook_listener();
        let (base, _origin) = started(&webhook);
        let key = "idempotency-key: order-1:press-a\r\n";
        let (code, _, made) = ask(
            &base,
            "POST",
            "/v1/payment_intents",
            key,
            r#"{"order":"order-1","owner":"reader-a","amount":1250}"#,
        );
        assert_eq!(code, 200, "{made}");
        let made: serde_json::Value = serde_json::from_str(&made).expect("json");
        assert_eq!(made["status"], "requires_payment_method");
        let (_, _, again) = ask(
            &base,
            "POST",
            "/v1/payment_intents",
            key,
            r#"{"order":"order-1","owner":"reader-a","amount":1250}"#,
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&again).expect("json")["id"],
            made["id"]
        );
        let (code, _, refused) = ask(
            &base,
            "POST",
            "/v1/payment_intents",
            key,
            r#"{"order":"order-1","owner":"reader-a","amount":9999}"#,
        );
        assert_eq!(code, 400, "{refused}");
        assert!(refused.contains("idempotency_error"));
        let (code, _, _) = ask(
            &base,
            "POST",
            "/v1/payment_intents",
            "",
            r#"{"order":"order-1","owner":"reader-a","amount":1250}"#,
        );
        assert_eq!(code, 400, "a request without a key is refused");

        // The hosted fields: a form, embedded by the host alone, no script.
        let secret = made["client_secret"].as_str().expect("secret");
        let (code, head, page) = ask(&base, "GET", &format!("/pay/{secret}"), "", "");
        assert_eq!(code, 200);
        assert!(page.contains("Pay $12.50"), "{page}");
        assert!(
            head.contains("frame-ancestors http://127.0.0.1:3301"),
            "{head}"
        );
        assert!(head.contains("default-src 'none'"), "{head}");

        // Declined, then paid: two events, signed for the host.
        let form = "content-type: application/x-www-form-urlencoded\r\n";
        let (_, _, declined) = ask(
            &base,
            "POST",
            &format!("/pay/{secret}"),
            form,
            "card=4000+0000+0000+0002",
        );
        assert!(declined.contains("Your card was declined."), "{declined}");
        let (_, _, paid) = ask(
            &base,
            "POST",
            &format!("/pay/{secret}"),
            form,
            "card=4242424242424242",
        );
        assert!(paid.contains("Paid."), "{paid}");
        let heard = heard.lock().expect("heard").clone();
        assert_eq!(heard.len(), 2, "{heard:?}");
        for (sig, body) in &heard {
            assert_eq!(
                verify(Some(sig), body.as_bytes(), b"whsec_test", 1_000_000),
                Ok(())
            );
        }
        assert!(heard[0].1.contains("payment_intent.payment_failed"));
        assert!(heard[1].1.contains("payment_intent.succeeded"));
        let (_, _, read) = ask(
            &base,
            "GET",
            &format!("/v1/payment_intents/{}", made["id"].as_str().expect("id")),
            "",
            "",
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&read).expect("json")["status"],
            "succeeded"
        );
    }

    /// **A provider may send an event twice, and out of order**: the test
    /// switches the host is held to.
    #[test]
    fn the_provider_can_send_twice_and_out_of_order() {
        let (webhook, heard) = webhook_listener();
        let (base, origin) = started(&webhook);
        let (_, _, made) = ask(
            &base,
            "POST",
            "/v1/payment_intents",
            "idempotency-key: k\r\n",
            r#"{"order":"o","owner":"reader-a","amount":500}"#,
        );
        let made: serde_json::Value = serde_json::from_str(&made).expect("json");
        let secret = made["client_secret"].as_str().expect("secret").to_string();
        let form = "content-type: application/x-www-form-urlencoded\r\n";
        origin.hold();
        ask(
            &base,
            "POST",
            &format!("/pay/{secret}"),
            form,
            "card=4000000000000002",
        );
        ask(
            &base,
            "POST",
            &format!("/pay/{secret}"),
            form,
            "card=4242424242424242",
        );
        assert!(heard.lock().expect("heard").is_empty(), "held");
        origin.twice.store(true, Ordering::SeqCst);
        origin.release_reversed();
        let kinds: Vec<String> = heard
            .lock()
            .expect("heard")
            .iter()
            .map(|(_, b)| {
                serde_json::from_str::<serde_json::Value>(b).expect("json")["type"]
                    .as_str()
                    .expect("type")
                    .to_string()
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "payment_intent.succeeded",
                "payment_intent.succeeded",
                "payment_intent.payment_failed",
                "payment_intent.payment_failed"
            ]
        );
    }
}

// --- the host, hearing the provider ---------------------------------------

/// **A GET of `url` through `client`**, the payments' own (track
/// outbound-https): its status and body. The client reaches the provider's
/// origin alone, and reads an answer by its length within `PROVIDER_WAIT`.
pub(crate) fn get(client: &Client, url: &str) -> Result<(u16, String), String> {
    let answer = client
        .send(&outbound::Request::get(url))
        .map_err(|e| e.to_string())?;
    Ok((answer.status(), answer.text()))
}

/// **The payment an owner's order awaits** (step seven): the order's
/// reference, which the payment carries, the provider's payment once begun,
/// and the provider's page for it, which the order page frames.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Awaiting {
    pub(crate) reference: String,
    pub(crate) payment: String,
    pub(crate) page: String,
}

/// **An intent as the provider says it now**, read again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settled {
    pub(crate) intent: String,
    pub(crate) order: String,
    /// Who began it, on whose behalf it is settled.
    pub(crate) owner: String,
    pub(crate) amount: i64,
    pub(crate) status: String,
}

/// **Why the host did not act on a webhook.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotActed {
    /// Not the provider's word (`verify`).
    Refused(Refused),
    /// The provider's word, heard before: answered, and nothing done again.
    HeardBefore,
    /// Signed, but no event the host reads, or one naming no intent the
    /// provider has.
    Unreadable(String),
    /// The intent's amount is not the one it was begun with: settled
    /// nothing, said.
    AmountChanged { begun: i64, now: i64 },
}

/// **What the host hears from its provider** (NEXT 6c). An event is acted
/// on only as the provider signed it, and then not as the event says but as
/// the provider says the intent is when asked again: an event may arrive
/// twice, or after a later one, and the intent's state now is what holds.
/// An event heard before is answered and ignored. What the host then does
/// is a transition that holds once (an order awaiting payment placed), so
/// an event heard twice across a restart changes nothing the second time;
/// the ids kept here spare the provider a second read.
pub(crate) struct Hearing {
    secret: Vec<u8>,
    /// The provider's API, where an intent is read again.
    api: String,
    /// The client the intent is read again with: the provider's origin
    /// alone.
    client: Client,
    heard: Mutex<std::collections::BTreeSet<String>>,
}

impl Hearing {
    pub(crate) fn new(secret: &[u8], api: &str, client: Client) -> Hearing {
        Hearing {
            secret: secret.to_vec(),
            api: api.trim_end_matches('/').to_string(),
            client,
            heard: Mutex::new(std::collections::BTreeSet::new()),
        }
    }

    /// **A webhook, heard at `now`**: the intent it names, as the provider
    /// says it now, where the host should act on it, with the event's id,
    /// which [`Hearing::remember`] keeps once the host has acted.
    pub(crate) fn webhook(
        &self,
        header: Option<&str>,
        body: &[u8],
        now: i64,
    ) -> Result<(String, Settled), NotActed> {
        verify(header, body, &self.secret, now).map_err(NotActed::Refused)?;
        let event: serde_json::Value =
            serde_json::from_slice(body).map_err(|e| NotActed::Unreadable(e.to_string()))?;
        let (Some(id), Some(intent)) =
            (event["id"].as_str(), event["data"]["object"]["id"].as_str())
        else {
            return Err(NotActed::Unreadable("no event id or intent".into()));
        };
        // Remembered only once acted on (step seven): an event the host could
        // not act on is acted on when the provider sends it again.
        if self.heard.lock().expect("heard").contains(id) {
            return Err(NotActed::HeardBefore);
        }
        let (code, read) = get(
            &self.client,
            &format!("{}/v1/payment_intents/{intent}", self.api),
        )
        .map_err(NotActed::Unreadable)?;
        if code != 200 {
            return Err(NotActed::Unreadable(format!(
                "the provider answered {code} for {intent}"
            )));
        }
        let read: serde_json::Value =
            serde_json::from_str(&read).map_err(|e| NotActed::Unreadable(e.to_string()))?;
        let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
        let amount = read["amount"].as_i64().unwrap_or_default();
        let begun = read["metadata"]["amount"]
            .as_str()
            .and_then(|a| a.parse::<i64>().ok())
            .ok_or_else(|| NotActed::Unreadable(format!("{intent} says no amount begun")))?;
        if amount != begun {
            return Err(NotActed::AmountChanged { begun, now: amount });
        }
        Ok((
            id.to_string(),
            Settled {
                intent: intent.to_string(),
                order: text(&read["metadata"]["order"]),
                owner: text(&read["metadata"]["owner"]),
                amount,
                status: text(&read["status"]),
            },
        ))
    }

    /// **An event acted on, remembered**: heard again, it is answered and
    /// nothing is done again.
    pub(crate) fn remember(&self, event: &str) {
        self.heard.lock().expect("heard").insert(event.to_string());
    }
}

#[cfg(test)]
mod hearing_tests {
    use super::*;

    fn provider() -> (String, Arc<Origin>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        // Its webhooks go nowhere here: each test hands the host the events
        // as the provider would send them.
        let origin = Arc::new(Origin::new(
            b"whsec_t",
            "http://127.0.0.1:9/x",
            "http://127.0.0.1:3301",
            || 5_000,
        ));
        let served = origin.clone();
        std::thread::spawn(move || serve(listener, served));
        (base, origin)
    }

    /// **An intent whose amount is not the one begun settles nothing**, said
    /// (step seven): the host holds a payment to the amount it began.
    #[test]
    fn an_intent_whose_amount_changed_at_the_provider_settles_nothing() {
        let (base, origin) = provider();
        let hearing = Hearing::new(
            b"whsec_t",
            &base,
            development_client(&base).expect("a client"),
        );
        {
            let mut p = origin.provider.lock().expect("provider");
            let i = p
                .create("k", "order-8", "reader-a", 1250, 5_000)
                .expect("made");
            p.amend(&i.id, 1).expect("amended");
            p.pay(&i.client_secret, "4242424242424242").expect("paid");
        }
        let events = std::mem::take(&mut origin.provider.lock().expect("provider").outbox);
        let (body, header) = DevProvider::sent(&events[0], b"whsec_t", 5_000);
        assert_eq!(
            hearing.webhook(Some(&header), body.as_bytes(), 5_000),
            Err(NotActed::AmountChanged {
                begun: 1250,
                now: 1
            })
        );
        // Control: unchanged, it is acted on.
        let mut p = origin.provider.lock().expect("provider");
        let i = p
            .create("k2", "order-9", "reader-a", 1250, 5_000)
            .expect("made");
        p.pay(&i.client_secret, "4242424242424242").expect("paid");
        let events = std::mem::take(&mut p.outbox);
        drop(p);
        let (body, header) = DevProvider::sent(&events[0], b"whsec_t", 5_000);
        assert!(
            hearing
                .webhook(Some(&header), body.as_bytes(), 5_000)
                .is_ok()
        );
    }

    /// **An outcome the host could not settle is settled when it comes
    /// again** (step seven): answered 500 and not remembered, so the
    /// provider's resend is acted on; once acted on, it is remembered, and
    /// heard again it is answered and not acted on twice.
    #[test]
    fn an_outcome_the_host_could_not_settle_is_settled_when_sent_again() {
        let (base, origin) = provider();
        let payments = Payments::new(
            &base,
            b"whsec_t",
            development_client(&base).expect("a client"),
        );
        {
            let mut p = origin.provider.lock().expect("provider");
            let i = p
                .create("k", "order-6", "reader-a", 1250, 5_000)
                .expect("made");
            p.pay(&i.client_secret, "4242424242424242").expect("paid");
        }
        let events = std::mem::take(&mut origin.provider.lock().expect("provider").outbox);
        let (body, header) = DevProvider::sent(&events[0], b"whsec_t", wall_seconds());
        let failing = |_: &Settled| Err("the store's layer is down".to_string());
        assert_eq!(
            payments.heard(Some(&header), body.as_bytes(), &failing).0,
            500
        );
        assert!(payments.settled.lock().expect("settled").is_empty());
        let settling = |s: &Settled| Ok(format!("{} settled", s.intent));
        assert_eq!(
            payments.heard(Some(&header), body.as_bytes(), &settling).0,
            200
        );
        assert_eq!(payments.settled.lock().expect("settled").len(), 1);
        let never = |_: &Settled| -> Result<String, String> { panic!("settled twice") };
        assert_eq!(
            payments.heard(Some(&header), body.as_bytes(), &never),
            (200, "heard before".to_string())
        );
    }

    /// **The host acts on the intent as the provider says it now**: a
    /// signed event is read again; heard twice, ignored the second time;
    /// forged or stale, refused; a failure that arrives after the success
    /// reads as succeeded, which is what the intent is.
    #[test]
    fn the_host_acts_on_the_intent_as_the_provider_says_it_now() {
        let (base, origin) = provider();
        let hearing = Hearing::new(
            b"whsec_t",
            &base,
            development_client(&base).expect("a client"),
        );
        let id = {
            let mut p = origin.provider.lock().expect("provider");
            let i = p
                .create("k", "order-7", "reader-a", 1250, 5_000)
                .expect("made");
            p.pay(&i.client_secret, "4000000000000002")
                .expect("declined");
            p.pay(&i.client_secret, "4242424242424242").expect("paid");
            i.id
        };
        let events = std::mem::take(&mut origin.provider.lock().expect("provider").outbox);
        let (failed, succeeded) = (&events[0], &events[1]);

        let (body, header) = DevProvider::sent(succeeded, b"whsec_t", 5_000);
        let (event, settled) = hearing
            .webhook(Some(&header), body.as_bytes(), 5_000)
            .expect("acted on");
        assert_eq!(
            settled,
            Settled {
                intent: id.clone(),
                order: "order-7".into(),
                owner: "reader-a".into(),
                amount: 1250,
                status: "succeeded".into()
            }
        );
        // Not yet acted on, it is heard again; remembered once it is.
        assert!(
            hearing
                .webhook(Some(&header), body.as_bytes(), 5_005)
                .is_ok()
        );
        hearing.remember(&event);
        assert_eq!(
            hearing.webhook(Some(&header), body.as_bytes(), 5_010),
            Err(NotActed::HeardBefore)
        );

        // The failure, delivered late: read again, the intent succeeded.
        let (body, header) = DevProvider::sent(failed, b"whsec_t", 5_000);
        let (_, late) = hearing
            .webhook(Some(&header), body.as_bytes(), 5_020)
            .expect("acted on");
        assert_eq!(late.status, "succeeded");

        // Not the provider's word.
        let forged = DevProvider::sent(
            &Event {
                id: "evt_x".into(),
                kind: "payment_intent.succeeded",
                intent: id.clone(),
            },
            b"whsec_other",
            5_000,
        );
        assert!(matches!(
            hearing.webhook(Some(&forged.1), forged.0.as_bytes(), 5_000),
            Err(NotActed::Refused(Refused::Forged))
        ));
        let stale = DevProvider::sent(
            &Event {
                id: "evt_y".into(),
                kind: "payment_intent.succeeded",
                intent: id,
            },
            b"whsec_t",
            4_000,
        );
        assert!(matches!(
            hearing.webhook(Some(&stale.1), stale.0.as_bytes(), 5_000),
            Err(NotActed::Refused(Refused::Stale { .. }))
        ));
        let unknown = DevProvider::sent(
            &Event {
                id: "evt_z".into(),
                kind: "payment_intent.succeeded",
                intent: "pi_none".into(),
            },
            b"whsec_t",
            5_000,
        );
        assert!(matches!(
            hearing.webhook(Some(&unknown.1), unknown.0.as_bytes(), 5_000),
            Err(NotActed::Unreadable(_))
        ));
    }
}

// --- the deployment's payments, configured ---------------------------------

/// **A header's value** among a request's raw headers, its name in any case.
pub(crate) fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// **The deployment's payments** (NEXT 6c): its provider's origin, and what
/// the host has heard from it.
pub(crate) struct Payments {
    /// The provider's origin, which a page embeds the hosted fields from.
    pub(crate) provider: String,
    /// **The client the host reaches its provider with** (track
    /// outbound-https): granted the provider's origin and nothing else, so
    /// no request the payments make reaches another; its whole exchange
    /// within `PROVIDER_WAIT`.
    client: Client,
    hearing: Hearing,
    /// Each payment the provider's signed word settled, as read again, in
    /// the order heard: what an order's transition reads (to come, with the
    /// order's total, NEXT 6b).
    pub(crate) settled: Mutex<Vec<Settled>>,
}

impl Payments {
    /// **The deployment's payments at `provider`**, heard with `secret`,
    /// reached with `client`, whose grant is the provider's origin.
    pub(crate) fn new(provider: &str, secret: &[u8], client: Client) -> Payments {
        Payments {
            provider: provider.trim_end_matches('/').to_string(),
            hearing: Hearing::new(secret, provider, client.clone()),
            client,
            settled: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn describe(&self) -> String {
        format!("the development provider, at {}", self.provider)
    }

    /// **A webhook, answered**: 200 where it is acted on or was heard
    /// before, as a provider stops sending an event it was answered for; 400
    /// where it is not the provider's word.
    pub(crate) fn heard(
        &self,
        signature: Option<&str>,
        body: &[u8],
        settle: &dyn Fn(&Settled) -> Result<String, String>,
    ) -> (u16, String) {
        match self.hearing.webhook(signature, body, wall_seconds()) {
            Ok((event, settled)) => match settle(&settled) {
                // Acted on: remembered, and answered so the provider stops.
                Ok(said) => {
                    self.hearing.remember(&event);
                    self.settled.lock().expect("settled").push(settled);
                    (200, said)
                }
                // Not acted on: not remembered, and answered so the
                // provider sends it again.
                Err(why) => (500, format!("not settled: {why}")),
            },
            Err(NotActed::HeardBefore) => (200, "heard before".to_string()),
            Err(NotActed::Refused(why)) => (400, format!("not the provider's word: {why:?}")),
            Err(NotActed::Unreadable(why)) => (400, format!("unreadable: {why}")),
            Err(NotActed::AmountChanged { begun, now }) => (
                400,
                format!("its amount is {now}, and it was begun with {begun}: settled nothing"),
            ),
        }
    }
}

/// A test's: the deployment's payments at a provider that is not there.
#[cfg(test)]
pub(crate) fn unreachable() -> Arc<Payments> {
    let provider = "http://127.0.0.1:9";
    let client = development_client(provider).expect("a development client");
    Arc::new(Payments::new(provider, b"x", client))
}

/// **The deployment's payments, as its configuration states them.**
/// `PW_PAYMENTS` unset (or empty) names none; `dev` names the development
/// provider, on an origin of its own at this server's port and 100
/// (`PW_PAYMENTS_PORT` otherwise), with a webhook secret made at start. The
/// development provider charges nothing, and runs only in a development
/// deployment on this machine (`local`).
pub(crate) fn configure(
    local: bool,
    host_origin: &str,
    port: u16,
    choice: Option<&str>,
    provider_port: Option<&str>,
) -> Result<Option<Arc<Payments>>, String> {
    match choice.unwrap_or("") {
        "" => Ok(None),
        "dev" => {
            if !local {
                return Err(format!(
                    "the development payment provider charges nothing, and runs only in a \
                     development deployment on this machine, not at `{host_origin}`"
                ));
            }
            let provider_port = match provider_port {
                Some(p) => p
                    .parse::<u16>()
                    .map_err(|_| format!("PW_PAYMENTS_PORT is `{p}`, not a port"))?,
                None => port
                    .checked_add(100)
                    .ok_or("no port 100 above this server's")?,
            };
            Ok(Some(development(host_origin, port, provider_port)?.0))
        }
        other => Err(format!(
            "PW_PAYMENTS is `{other}`: this server knows `dev`, or none"
        )),
    }
}

/// **The development provider, started** on an origin of its own at
/// `provider_port`, telling the host at `port` (NEXT 6c): the deployment's
/// payments, and the provider, which a test holds to send its events late.
pub(crate) fn development(
    host_origin: &str,
    port: u16,
    provider_port: u16,
) -> Result<(Arc<Payments>, Arc<Origin>), String> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| format!("a webhook secret: {e}"))?;
    let listener = TcpListener::bind(("127.0.0.1", provider_port))
        .map_err(|e| format!("the development payment provider's port {provider_port}: {e}"))?;
    let provider = format!("http://127.0.0.1:{provider_port}");
    let origin = Arc::new(Origin::new(
        &secret,
        &format!("http://127.0.0.1:{port}/payments/webhook"),
        host_origin,
        wall_seconds,
    ));
    let served = origin.clone();
    std::thread::spawn(move || serve(listener, served));
    let payments = Arc::new(Payments::new(
        &provider,
        &secret,
        development_client(&provider)?,
    ));
    Ok((payments, origin))
}

/// **The client a development deployment reaches its development provider
/// with** (track outbound-https): the provider's origin granted as a
/// development one, plain HTTP on this machine and nowhere else, and no
/// roots, so that no https request it made would be trusted. A deployment
/// whose provider is at a public origin grants it as one (`Grant::public`),
/// over TLS against the platform's roots (`Roots::platform`): NEXT 6d's.
pub(crate) fn development_client(provider: &str) -> Result<Client, String> {
    let grant = outbound::Grant::none()
        .development(provider)
        .map_err(|e| e.to_string())?;
    Ok(Client::new(grant, outbound::Roots::none())
        .map_err(|e| e.to_string())?
        .with_limits(provider_limits()))
}

/// **How long, and how much, the host waits for its provider**: the whole
/// exchange within `PROVIDER_WAIT` (step eight), connecting and the
/// handshake within it.
pub(crate) fn provider_limits() -> outbound::Limits {
    outbound::Limits {
        connect: PROVIDER_WAIT,
        handshake: PROVIDER_WAIT,
        total: PROVIDER_WAIT,
        ..outbound::Limits::default()
    }
}

#[cfg(test)]
mod configured_tests {
    use super::*;

    /// **The development provider runs only on this machine, in
    /// development**, and a choice this server does not know is refused.
    #[test]
    fn the_development_provider_runs_only_in_development_here() {
        assert!(
            configure(true, "http://127.0.0.1:3301", 3301, None, None)
                .expect("none")
                .is_none()
        );
        let refused = configure(false, "https://store.example", 3301, Some("dev"), None)
            .err()
            .expect("refused");
        assert!(
            refused.contains("runs only in a development deployment"),
            "{refused}"
        );
        let unknown = configure(true, "http://127.0.0.1:3301", 3301, Some("stripe"), None)
            .err()
            .expect("refused");
        assert!(unknown.contains("PW_PAYMENTS is `stripe`"), "{unknown}");
    }

    /// **A payment through the configured provider reaches the host's
    /// webhook, and the host acts on it**: the provider and the host share
    /// the secret made at start, and nothing else does.
    #[test]
    fn the_configured_provider_and_host_agree_on_their_secret() {
        // A host listening, to hear the provider's webhook as `main` would.
        let host = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = host.local_addr().expect("addr").port();
        // The provider on a port of its own, free now.
        let free = TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port()
            .to_string();
        let payments = configure(
            true,
            &format!("http://127.0.0.1:{port}"),
            port,
            Some("dev"),
            Some(&free),
        )
        .expect("configured")
        .expect("payments");
        let heard = payments.clone();
        std::thread::spawn(move || {
            for stream in host.incoming() {
                let Ok(mut stream) = stream else { continue };
                if let Some(req) = read_request(&stream) {
                    let (code, said) =
                        heard.heard(req.header("stripe-signature"), &req.body, &|s| {
                            Ok(format!("{} is {}", s.intent, s.status))
                        });
                    write(&mut stream, code, "text/plain", "", said.as_bytes());
                }
            }
        });
        let made = {
            let (code, body) = {
                let url = format!("{}/v1/payment_intents", payments.provider);
                let rest = url.strip_prefix("http://").expect("http");
                let (authority, path) = rest.split_once('/').expect("path");
                let mut stream = TcpStream::connect(authority).expect("connect");
                let body = r#"{"order":"order-1","owner":"reader-a","amount":1250}"#;
                let head = format!(
                    "POST /{path} HTTP/1.1\r\nhost: {authority}\r\nidempotency-key: k\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).expect("write");
                stream.write_all(body.as_bytes()).expect("write");
                let mut all = String::new();
                stream.read_to_string(&mut all).expect("read");
                let (head, body) = all.split_once("\r\n\r\n").expect("head");
                (
                    head.split_whitespace().nth(1).unwrap_or("").to_string(),
                    body.to_string(),
                )
            };
            assert_eq!(code, "200", "{body}");
            serde_json::from_str::<serde_json::Value>(&body).expect("json")
        };
        let secret = made["client_secret"].as_str().expect("secret");
        let url = format!("{}/pay/{secret}", payments.provider);
        let rest = url.strip_prefix("http://").expect("http");
        let (authority, path) = rest.split_once('/').expect("path");
        let mut stream = TcpStream::connect(authority).expect("connect");
        let body = "card=4242424242424242";
        let head = format!(
            "POST /{path} HTTP/1.1\r\nhost: {authority}\r\ncontent-type: application/x-www-form-urlencoded\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).expect("write");
        stream.write_all(body.as_bytes()).expect("write");
        let mut page = String::new();
        stream.read_to_string(&mut page).expect("read");
        assert!(page.contains("Paid."), "{page}");
        // The webhook is sent after the page is answered: wait for the host
        // to have heard it.
        let mut tries = 0;
        while payments.settled.lock().expect("settled").is_empty() && tries < 100 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            tries += 1;
        }
        let settled = payments.settled.lock().expect("settled").clone();
        assert_eq!(settled.len(), 1, "{settled:?}");
        assert_eq!(
            (
                settled[0].order.as_str(),
                settled[0].status.as_str(),
                settled[0].amount
            ),
            ("order-1", "succeeded", 1250)
        );
    }
}

// --- the program's operations ----------------------------------------------

use pw_host::engine::{HostFn, Val};
use pw_outbound::{self as outbound, Client};

/// **How long the host waits for its provider** (step eight): to connect,
/// to send and to be answered, all of it (track outbound-https: until then
/// each read was bounded, and not the answer). A host's commands wait while
/// the provider answers one of them, its layer held, so a provider that does
/// not answer in ten seconds is one that cannot be reached
/// (`TransportFailed`).
const PROVIDER_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// **A POST of JSON to `url` through `client`**, with an
/// `Idempotency-Key`: its status and body. A key the client would not write
/// (a line break in an interaction's id) sends nothing.
fn post_json(
    client: &Client,
    url: &str,
    key: &str,
    body: &serde_json::Value,
) -> Result<(u16, String), String> {
    let request = outbound::Request::post(url, "application/json", body.to_string())
        .header("idempotency-key", key);
    let answer = client.send(&request).map_err(|e| e.to_string())?;
    Ok((answer.status(), answer.text()))
}

/// A payment as the program's `Payment` is: `{ id, client_secret, status }`.
fn payment_val(provider: &str, intent: &serde_json::Value) -> Result<Val, String> {
    let text = |k: &str| {
        intent[k]
            .as_str()
            .map(|s| Val::String(s.to_string()))
            .ok_or_else(|| format!("the provider's intent has no `{k}`"))
    };
    let status = match intent["status"].as_str() {
        Some("requires_payment_method") => "requires-payment-method",
        Some("requires_action") => "requires-action",
        Some("processing") => "processing",
        Some("succeeded") => "succeeded",
        Some("canceled") => "canceled",
        other => return Err(format!("the provider's intent is {other:?}")),
    };
    let page = format!(
        "{}/pay/{}",
        provider.trim_end_matches('/'),
        intent["client_secret"].as_str().unwrap_or_default()
    );
    Ok(Val::Record(vec![
        ("id".into(), text("id")?),
        ("client-secret".into(), text("client_secret")?),
        ("status".into(), Val::Variant(status.into(), None)),
        // The provider's page for it, which the reader's page frames.
        ("page".into(), Val::String(page)),
    ]))
}

/// **The payments' operations where the deployment takes none**: `begin`
/// answers `NotTaken`, and the program places an order as it did before
/// payments (step seven).
pub(crate) fn not_taken() -> Vec<(String, HostFn)> {
    let begin: HostFn = Arc::new(|_args: &[Val]| {
        Ok(vec![Val::Result(Err(Some(Box::new(Val::Variant(
            "not-taken".into(),
            None,
        )))))])
    });
    // A program that cancels a payment where none is taken has named one no
    // payment was begun for.
    let cancel: HostFn = Arc::new(|_args: &[Val]| {
        Err("this deployment takes no payments: there is none to cancel".to_string())
    });
    vec![
        ("pw:host/payments#begin".to_string(), begin),
        ("pw:host/payments#cancel".to_string(), cancel),
    ]
}

/// **The payments' operations, for one command** (NEXT 6c):
/// `pw:host/payments#begin(order, amount)`, an intent for the order's
/// amount at the provider, keyed by the order and the command's interaction,
/// so a retried press reaches the intent the first made and never a second.
/// A provider that cannot be reached is the program's `TransportFailed`.
pub(crate) fn operations(
    payments: Arc<Payments>,
    interaction: Option<String>,
    owner: String,
) -> Vec<(String, HostFn)> {
    let (provider, reader) = (payments.provider.clone(), owner.clone());
    let client = payments.client.clone();
    let begin: HostFn = Arc::new(move |args: &[Val]| {
        let [Val::String(order), Val::S64(amount)] = args else {
            return Err(format!("payments#begin(order, amount) received {args:?}"));
        };
        let Some(interaction) = interaction.as_deref() else {
            return Err(
                "a payment is begun by an interaction: its command is `idempotent_by \
                 InteractionId`"
                    .to_string(),
            );
        };
        let asked = serde_json::json!({ "order": order, "owner": owner, "amount": amount });
        let failed = || {
            Ok(vec![Val::Result(Err(Some(Box::new(Val::Variant(
                "transport-failed".into(),
                None,
            )))))])
        };
        match post_json(
            &payments.client,
            &format!("{}/v1/payment_intents", payments.provider),
            &format!("{order}:{interaction}"),
            &asked,
        ) {
            Ok((200, body)) => {
                let intent: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| e.to_string())?;
                Ok(vec![Val::Result(Ok(Some(Box::new(payment_val(
                    &payments.provider,
                    &intent,
                )?))))])
            }
            // The key sent with other arguments: the program's own error,
            // as a command's interaction is (ADR-0121).
            Ok((400, body)) if body.contains("idempotency_error") => Err(format!(
                "the order's payment was begun with another amount: {body}"
            )),
            Ok(_) | Err(_) => failed(),
        }
    });
    // **A payment canceled, by its reader alone** (step eight): read first,
    // and refused where another reader began it, as no program of this
    // reader's could name one; canceled already, what was asked holds.
    let cancel: HostFn = Arc::new(move |args: &[Val]| {
        let [Val::String(payment)] = args else {
            return Err(format!("payments#cancel(payment) received {args:?}"));
        };
        let answer = |r: Result<Val, Val>| -> Result<Vec<Val>, String> {
            Ok(vec![Val::Result(match r {
                Ok(v) => Ok(Some(Box::new(v))),
                Err(e) => Err(Some(Box::new(e))),
            })])
        };
        let unreached = Val::Variant("transport-failed".into(), None);
        let canceled = Val::Variant("canceled".into(), None);
        let intent = format!("{provider}/v1/payment_intents/{payment}");
        let read: serde_json::Value = match get(&client, &intent) {
            Ok((200, body)) => serde_json::from_str(&body).map_err(|e| e.to_string())?,
            Ok((404, _)) => return Err(format!("no payment `{payment}` at the provider")),
            Ok(_) | Err(_) => return answer(Err(unreached)),
        };
        if read["metadata"]["owner"].as_str() != Some(reader.as_str()) {
            return Err(format!(
                "payment `{payment}` is not this reader's to cancel"
            ));
        }
        let asked = serde_json::json!({ "cancellation_reason": "requested_by_customer" });
        match post_json(
            &client,
            &format!("{intent}/cancel"),
            &format!("cancel-{payment}"),
            &asked,
        ) {
            Ok((200, _)) => answer(Ok(canceled)),
            Ok((400, body)) => {
                let said: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| e.to_string())?;
                match said["error"]["payment_intent"]["status"].as_str() {
                    Some("canceled") => answer(Ok(canceled)),
                    Some(status) => answer(Err(Val::Variant(
                        "past".into(),
                        Some(Box::new(Val::Variant(status.replace('_', "-"), None))),
                    ))),
                    None => Err(format!("the provider refused the cancel: {body}")),
                }
            }
            Ok(_) | Err(_) => answer(Err(unreached)),
        }
    });
    vec![
        ("pw:host/payments#begin".to_string(), begin),
        ("pw:host/payments#cancel".to_string(), cancel),
    ]
}

/// **Whether the platform invokes `component_id`, and no reader** (NEXT
/// 6c): its export is `invoked_by` a platform invoker (`Payments`). The
/// press route answers a press naming it as a command that is not there.
pub(crate) fn invoked_by_platform(
    contracts: &[pw_host::ComponentContract],
    component_id: &str,
) -> bool {
    contracts
        .iter()
        .find(|c| c.component_id == component_id)
        .and_then(|c| c.exports.first()?.component.as_ref())
        .is_some_and(|e| e.invoked_by.is_some())
}

#[cfg(test)]
mod invoked_tests {
    use super::*;
    use pw_host::{BindingSupport, CAPABILITY_MAPPING, ComponentContract, ComponentExport, Export};

    fn contract(id: &str, invoked_by: Option<&str>) -> ComponentContract {
        ComponentContract {
            component_id: id.into(),
            capability_mapping: CAPABILITY_MAPPING,
            abi_schema: "abi".into(),
            required_capabilities: Vec::new(),
            allowed_placements: vec!["origin".into()],
            imports: Vec::new(),
            exports: vec![Export {
                name: "settle".into(),
                kind: "command".into(),
                binding: BindingSupport::default(),
                component: Some(ComponentExport {
                    interface: "pw:app/store-page-settle-api@0.1.0".into(),
                    function: "settle".into(),
                    authorization: Vec::new(),
                    idempotent_by: None,
                    invoked_by: invoked_by.map(str::to_string),
                    bounded: Vec::new(),
                }),
            }],
        }
    }

    /// **A command the platform invokes is no reader's to press**, by its
    /// contract; a reader's command, and a component this host does not
    /// have, are not one.
    #[test]
    fn a_command_the_platform_invokes_is_told_by_its_contract() {
        let contracts = [
            contract("store.page.settle", Some("Payments")),
            contract("store.page.place_order", None),
        ];
        assert!(invoked_by_platform(&contracts, "store.page.settle"));
        assert!(!invoked_by_platform(&contracts, "store.page.place_order"));
        assert!(!invoked_by_platform(&contracts, "store.page.nothing"));
    }
}

#[cfg(test)]
mod operation_tests {
    use super::*;

    fn configured() -> Arc<Payments> {
        let free = TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port()
            .to_string();
        configure(
            true,
            "http://127.0.0.1:3301",
            3301,
            Some("dev"),
            Some(&free),
        )
        .expect("configured")
        .expect("payments")
    }

    /// **A payment begun once per interaction**: the same order's press
    /// sent again reaches the same intent; another interaction is another
    /// intent; no interaction is refused, as is the same press with another
    /// amount; a provider that cannot be reached is `TransportFailed`.
    #[test]
    fn a_payment_is_begun_once_per_interaction() {
        let payments = configured();
        let begin = |interaction: Option<&str>, amount: i64| {
            let ops = operations(
                payments.clone(),
                interaction.map(str::to_string),
                "reader-a".into(),
            );
            ops[0].1(&[Val::String("order-1".into()), Val::S64(amount)])
        };
        let id = |out: Vec<Val>| match out.as_slice() {
            [Val::Result(Ok(Some(p)))] => match p.as_ref() {
                Val::Record(fields) => match &fields[0].1 {
                    Val::String(id) => id.clone(),
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        };
        let first = id(begin(Some("press-a"), 1250).expect("begun"));
        assert_eq!(
            id(begin(Some("press-a"), 1250).expect("begun again")),
            first
        );
        assert_ne!(id(begin(Some("press-b"), 1250).expect("another")), first);
        assert!(
            begin(None, 1250)
                .expect_err("no interaction")
                .contains("idempotent_by")
        );
        assert!(
            begin(Some("press-a"), 999)
                .expect_err("another amount")
                .contains("another amount")
        );
        // A provider that is not there.
        let out = operations(unreachable(), Some("press-a".into()), "reader-a".into())[0].1(&[
            Val::String("order-1".into()),
            Val::S64(1250),
        ])
        .expect("answered");
        assert_eq!(
            out,
            vec![Val::Result(Err(Some(Box::new(Val::Variant(
                "transport-failed".into(),
                None
            )))))]
        );
    }
}
#[cfg(test)]
mod answer_tests {
    use super::*;

    /// **An answer is read by its length** (step eight): its body read once
    /// it is sent, while the connection stays open. Read to the close, a
    /// provider that sends its event before closing held a reader's
    /// command, which held the host's layer the event waited for.
    #[test]
    fn an_answer_is_read_by_its_length_not_its_close() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/v1/x", listener.local_addr().expect("addr"));
        let (done, held) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request(&stream);
            write(&mut stream, 200, "application/json", "", br#"{"ok":true}"#);
            // Open until the answer is read, or thirty seconds.
            let _ = held.recv_timeout(std::time::Duration::from_secs(30));
        });
        let base = url.trim_end_matches("/v1/x").to_string();
        let client = development_client(&base).expect("a client");
        let started = std::time::Instant::now();
        let answered = get(&client, &url);
        let took = started.elapsed();
        let _ = done.send(());
        assert_eq!(answered, Ok((200, r#"{"ok":true}"#.to_string())));
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
    }

    /// **A provider that does not answer in time is one that cannot be
    /// reached** (step eight): the host's commands wait while it answers, so
    /// they wait no longer than `PROVIDER_WAIT`, the whole exchange (track
    /// outbound-https). Here a tenth of a second, against a provider that
    /// never answers.
    #[test]
    fn a_provider_that_does_not_answer_in_time_is_not_reached() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let (done, held) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("accept");
            let _ = held.recv_timeout(std::time::Duration::from_secs(30));
        });
        let client = development_client(&base)
            .expect("a client")
            .with_limits(outbound::Limits {
                total: std::time::Duration::from_millis(100),
                ..provider_limits()
            });
        let started = std::time::Instant::now();
        let answered = get(&client, &format!("{base}/v1/x"));
        let took = started.elapsed();
        let _ = done.send(());
        assert!(answered.is_err(), "{answered:?}");
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
        assert_eq!(PROVIDER_WAIT, std::time::Duration::from_secs(10));
        let limits = development_client(&base).expect("a client").limits();
        assert_eq!(
            (limits.connect, limits.handshake, limits.total),
            (PROVIDER_WAIT, PROVIDER_WAIT, PROVIDER_WAIT)
        );
    }
}

#[cfg(test)]
mod cancel_tests {
    use super::*;

    /// The operations of a command of `reader`'s, with an interaction.
    fn of(payments: &Arc<Payments>, reader: &str, press: &str) -> Vec<(String, HostFn)> {
        operations(payments.clone(), Some(press.into()), reader.into())
    }

    fn begun(payments: &Arc<Payments>, reader: &str, order: &str) -> String {
        let out =
            of(payments, reader, "press-a")[0].1(&[Val::String(order.into()), Val::S64(1250)])
                .expect("begun");
        match out.as_slice() {
            [Val::Result(Ok(Some(p)))] => match p.as_ref() {
                Val::Record(fields) => match &fields[0].1 {
                    Val::String(id) => id.clone(),
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    fn canceled(payments: &Arc<Payments>, reader: &str, id: &str) -> Result<Vec<Val>, String> {
        of(payments, reader, "press-b")[1].1(&[Val::String(id.into())])
    }

    /// **A payment is canceled by its reader alone** (step eight): canceled,
    /// and again, `Canceled`; another reader's refused before the provider
    /// is asked; one paid, `Past(Succeeded)`; a provider not there,
    /// `TransportFailed`.
    #[test]
    fn a_payment_is_canceled_by_its_reader_alone() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let host_port = listener.local_addr().expect("addr").port();
        drop(listener);
        let free = TcpListener::bind("127.0.0.1:0").expect("bind");
        let provider_port = free.local_addr().expect("addr").port();
        drop(free);
        let (payments, origin) =
            development("http://127.0.0.1:3301", host_port, provider_port).expect("started");
        // Its events go to no host here.
        origin.hold();
        let ok_canceled = vec![Val::Result(Ok(Some(Box::new(Val::Variant(
            "canceled".into(),
            None,
        )))))];
        let id = begun(&payments, "reader-a", "o-1");
        assert_eq!(
            canceled(&payments, "reader-a", &id),
            Ok(ok_canceled.clone())
        );
        assert_eq!(canceled(&payments, "reader-a", &id), Ok(ok_canceled));
        let other = begun(&payments, "reader-a", "o-2");
        assert!(
            canceled(&payments, "reader-b", &other)
                .expect_err("another reader's")
                .contains("not this reader's")
        );
        assert_eq!(
            origin
                .provider
                .lock()
                .expect("provider")
                .read(&other)
                .map(|i| i.status),
            Ok(Status::RequiresPaymentMethod),
            "refused before the provider was asked"
        );
        let paid = begun(&payments, "reader-a", "o-3");
        {
            let mut p = origin.provider.lock().expect("provider");
            let secret = p.read(&paid).expect("read").client_secret;
            p.pay(&secret, "4242424242424242").expect("paid");
        }
        assert_eq!(
            canceled(&payments, "reader-a", &paid),
            Ok(vec![Val::Result(Err(Some(Box::new(Val::Variant(
                "past".into(),
                Some(Box::new(Val::Variant("succeeded".into(), None))),
            )))))])
        );
        assert_eq!(
            operations(unreachable(), Some("press-a".into()), "reader-a".into())[1].1(&[
                Val::String(id.clone())
            ]),
            Ok(vec![Val::Result(Err(Some(Box::new(Val::Variant(
                "transport-failed".into(),
                None
            )))))])
        );
    }
}

/// **The payments at an https origin** (track outbound-https): the same
/// operations and the same hearing, through the same client, over TLS. The
/// development provider stands behind a TLS front presenting a certificate
/// of a test's authority, at a name fixed to this machine, as a provider
/// stands behind its load balancer; a deployment's provider at a public
/// origin differs in its grant (`Grant::public`) and its roots
/// (`Roots::platform`) alone.
#[cfg(test)]
mod https_tests {
    use super::*;
    use pw_outbound::testing::{Leaf, Server, TestCa};
    use std::net::{IpAddr, Ipv4Addr};

    const NAME: &str = "payments.provider.test";

    /// The development provider on a port of its own, its events held,
    /// behind a TLS front presenting `leaf`: the provider, the front, and
    /// the front's origin.
    fn behind_tls(leaf: &Leaf) -> (Arc<Origin>, Server, String) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let backend = listener.local_addr().expect("addr");
        let origin = Arc::new(Origin::new(
            b"whsec_t",
            "http://127.0.0.1:9/x",
            "http://127.0.0.1:3301",
            wall_seconds,
        ));
        origin.hold();
        let served = origin.clone();
        std::thread::spawn(move || serve(listener, served));
        let front = Server::front(leaf, backend);
        let https = format!("https://{NAME}:{}", front.port());
        (origin, front, https)
    }

    /// The payments at `https`, its client trusting `authority`'s root
    /// alone, the provider's name fixed to this machine.
    fn payments_at(https: &str, authority: &TestCa) -> Result<Arc<Payments>, String> {
        let grant = outbound::Grant::none()
            .development(https)
            .map_err(|e| e.to_string())?;
        let roots = outbound::Roots::only(&[authority.der()]).map_err(|e| e.to_string())?;
        let client = Client::new(grant, roots)
            .map_err(|e| e.to_string())?
            .with_resolver(Arc::new(
                outbound::Fixed::new().name(NAME, &[IpAddr::V4(Ipv4Addr::LOCALHOST)]),
            ))
            .with_limits(provider_limits());
        Ok(Arc::new(Payments::new(https, b"whsec_t", client)))
    }

    /// A payment begun for `order` by reader-a: its id, or what was said.
    fn begin(payments: &Arc<Payments>, order: &str, press: &str) -> Result<String, String> {
        let out = operations(payments.clone(), Some(press.into()), "reader-a".into())[0].1(&[
            Val::String(order.into()),
            Val::S64(1250),
        ])?;
        match out.as_slice() {
            [Val::Result(Ok(Some(p)))] => match p.as_ref() {
                Val::Record(fields) => match &fields[0].1 {
                    Val::String(id) => Ok(id.clone()),
                    other => Err(format!("{other:?}")),
                },
                other => Err(format!("{other:?}")),
            },
            other => Err(format!("{other:?}")),
        }
    }

    /// **A provider at an https origin is reached over TLS, its certificate
    /// verified**: a payment begun, its outcome heard and read again, and
    /// another canceled by its reader, every request through the front.
    #[test]
    fn a_provider_at_an_https_origin_is_reached_over_tls() {
        let authority = TestCa::new("payments test authority");
        let (origin, front, https) = behind_tls(&authority.server(&[NAME]));
        let payments = payments_at(&https, &authority);
        assert!(payments.is_ok(), "{:?}", payments.as_ref().err());
        let payments = payments.unwrap();
        let paid = begin(&payments, "o-1", "press-a");
        assert!(paid.is_ok(), "{paid:?}");
        let paid = paid.unwrap();
        let events = {
            let mut p = origin.provider.lock().expect("provider");
            let secret = p.read(&paid).map(|i| i.client_secret);
            assert!(secret.is_ok(), "{secret:?}");
            let charged = p.pay(&secret.unwrap(), "4242424242424242");
            assert!(charged.is_ok(), "{charged:?}");
            std::mem::take(&mut p.outbox)
        };
        let (body, header) = DevProvider::sent(&events[0], b"whsec_t", wall_seconds());
        let heard = payments
            .hearing
            .webhook(Some(&header), body.as_bytes(), wall_seconds())
            .map(|(_, s)| (s.order, s.owner, s.status));
        assert_eq!(
            heard,
            Ok(("o-1".into(), "reader-a".into(), "succeeded".into()))
        );
        let other = begin(&payments, "o-2", "press-b");
        assert!(other.is_ok(), "{other:?}");
        let canceled =
            operations(payments.clone(), Some("press-c".into()), "reader-a".into())[1].1(&[
                Val::String(other.unwrap()),
            ]);
        assert_eq!(
            canceled,
            Ok(vec![Val::Result(Ok(Some(Box::new(Val::Variant(
                "canceled".into(),
                None
            )))))])
        );
        let seen = front.seen();
        let host = format!("{NAME}:{}", front.port());
        assert_eq!(
            seen.iter()
                .map(|s| (s.head.split(' ').next().unwrap_or(""), s.header("host")))
                .collect::<Vec<_>>(),
            vec![
                ("POST", Some(host.as_str())),
                ("GET", Some(host.as_str())),
                ("POST", Some(host.as_str())),
                ("GET", Some(host.as_str())),
                ("POST", Some(host.as_str())),
            ]
        );
    }

    /// **A provider presenting a certificate the client does not trust is
    /// not reached**: the payment is `TransportFailed`, and the provider is
    /// asked nothing.
    #[test]
    fn a_provider_whose_certificate_is_not_trusted_is_not_reached() {
        let authority = TestCa::new("payments test authority");
        let stranger = TestCa::new("another authority");
        let (_origin, front, https) = behind_tls(&stranger.server(&[NAME]));
        let payments = payments_at(&https, &authority);
        assert!(payments.is_ok(), "{:?}", payments.as_ref().err());
        let out = operations(payments.unwrap(), Some("press-a".into()), "reader-a".into())[0].1(&[
            Val::String("o-1".into()),
            Val::S64(1250),
        ]);
        assert_eq!(
            out,
            Ok(vec![Val::Result(Err(Some(Box::new(Val::Variant(
                "transport-failed".into(),
                None
            )))))])
        );
        assert!(front.seen().is_empty(), "{:?}", front.seen());
    }

    /// **A reader's interaction that would split the provider's request is
    /// never sent**: the interaction keys the payment (`Idempotency-Key`),
    /// and is the reader's runtime's to name; a line break in it would end
    /// the field and begin another in the request to the provider. The
    /// client refuses the field, so the payment is `TransportFailed` and the
    /// provider is asked nothing. (Until the one client, it was written into
    /// the head as it came.)
    #[test]
    fn an_interaction_that_would_split_the_providers_request_is_never_sent() {
        let authority = TestCa::new("payments test authority");
        let (_origin, front, https) = behind_tls(&authority.server(&[NAME]));
        let payments = payments_at(&https, &authority);
        assert!(payments.is_ok(), "{:?}", payments.as_ref().err());
        let payments = payments.unwrap();
        for interaction in ["press-a\r\nx-injected: 1", "press-a\rb", "press-\u{e9}"] {
            let out = operations(
                payments.clone(),
                Some(interaction.into()),
                "reader-a".into(),
            )[0]
            .1(&[Val::String("o-1".into()), Val::S64(1250)]);
            assert_eq!(
                out,
                Ok(vec![Val::Result(Err(Some(Box::new(Val::Variant(
                    "transport-failed".into(),
                    None
                )))))]),
                "{interaction:?}"
            );
        }
        assert!(front.seen().is_empty(), "{:?}", front.seen());
        let sent = begin(&payments, "o-1", "press-a");
        assert!(sent.is_ok(), "{sent:?}");
    }

    /// **The payments reach their provider and nothing else**: their
    /// client's grant is the provider's origin, a development one where the
    /// provider is the development provider, and any other origin is
    /// refused before anything is sent.
    #[test]
    fn the_payments_reach_their_provider_alone() {
        let free = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = free.local_addr().expect("addr").port();
        drop(free);
        let configured = configure(
            true,
            "http://127.0.0.1:3301",
            3301,
            Some("dev"),
            Some(&port.to_string()),
        );
        assert!(
            matches!(configured, Ok(Some(_))),
            "{:?}",
            configured.as_ref().err()
        );
        let payments = configured.unwrap().unwrap();
        assert_eq!(
            payments.client.grant().origins(),
            vec![format!("http://127.0.0.1:{port}")]
        );
        assert_eq!(payments.provider, format!("http://127.0.0.1:{port}"));
        for elsewhere in [
            format!("http://127.0.0.1:{}/v1/payment_intents", port + 1),
            format!("https://127.0.0.1:{port}/v1/payment_intents"),
            format!("http://localhost:{port}/v1/payment_intents"),
        ] {
            let refused = payments
                .client
                .send(&outbound::Request::get(elsewhere.clone()));
            assert!(
                matches!(refused, Err(outbound::Error::Undeclared { .. })),
                "{elsewhere}: {refused:?}"
            );
        }
    }
}
