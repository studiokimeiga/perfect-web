//! **The client against servers of its own** (`pw-outbound-testkit`): the
//! grant, the addresses, the certificates, the redirects, the deadline, the
//! sizes and the secrets, each with the case it holds and its neighbour that
//! passes.

use std::fmt::Debug;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::*;
use pw_outbound_testkit::{self as testing, DAY_SECONDS, Leaf, Reply, Server, TestCa};

/// **The largest allocation each thread asks for**: the test binary's
/// allocator is the system's, every request noted, so that a test can know a
/// stated length was not allocated on a peer's word (a panic or an abort in
/// the allocator would say less, and not at the test's assertion).
pub(crate) mod allocations {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        // A `Cell` of a `usize`, initialized constantly: reading it
        // allocates nothing and registers no destructor.
        static LARGEST: Cell<usize> = const { Cell::new(0) };
    }

    fn note(size: usize) {
        let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
    }

    /// The largest allocation this thread has asked for since it last
    /// asked, the count begun again.
    pub(crate) fn largest() -> usize {
        LARGEST.try_with(|largest| largest.replace(0)).unwrap_or(0)
    }

    /// The system's allocator, noting each size it is asked for.
    pub(crate) struct Noting;

    // SAFETY: every call is the system allocator's, with the caller's own
    // arguments; noting a size allocates nothing.
    unsafe impl GlobalAlloc for Noting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            note(layout.size());
            // SAFETY: the caller's contract, passed on whole.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            note(layout.size());
            // SAFETY: as `alloc`.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            note(new_size);
            // SAFETY: as `alloc`.
            unsafe { System.realloc(ptr, layout, new_size) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: as `alloc`.
            unsafe { System.dealloc(ptr, layout) }
        }
    }
}

#[global_allocator]
static NOTING: allocations::Noting = allocations::Noting;

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const NAME: &str = "api.provider.test";
const OTHER: &str = "other.provider.test";
const SECRET: &str = "Bearer sk_test_51QvYk3ZpL0Secret";

fn ca() -> &'static TestCa {
    static CA: OnceLock<TestCa> = OnceLock::new();
    CA.get_or_init(|| TestCa::new("Pleris outbound test CA"))
}

/// What a result holds, where the test needs it to hold one.
fn held<T: Debug>(result: Result<T, Error>) -> T {
    assert!(result.is_ok(), "{result:?}");
    result.unwrap()
}

fn quick() -> Limits {
    Limits {
        connect: Duration::from_secs(2),
        handshake: Duration::from_secs(2),
        total: Duration::from_secs(4),
        head: 16 * 1024,
        body: 64 * 1024,
    }
}

fn https(name: &str, port: u16) -> String {
    format!("https://{name}:{port}")
}

/// A client of `grant`, trusting the test's authority, its names (each
/// pointing to `addresses`) fixed.
fn client(grant: Grant, roots: Roots, names: &[(&str, &[IpAddr])]) -> Client {
    let fixed = names.iter().fold(Fixed::new(), |fixed, (name, addresses)| {
        fixed.name(name, addresses)
    });
    held(Client::new(grant, roots))
        .with_resolver(Arc::new(fixed))
        .with_limits(quick())
}

fn trusting() -> Roots {
    held(Roots::only(&[ca().der()]))
}

/// A client granted `https://{NAME}:{port}` as a development origin,
/// trusting the test's authority, `NAME` and `OTHER` on this machine.
fn granted(port: u16) -> Client {
    client(
        held(Grant::none().development(&https(NAME, port))),
        trusting(),
        &[(NAME, &[LOOPBACK]), (OTHER, &[LOOPBACK])],
    )
}

fn serving(leaf: &Leaf) -> Server {
    Server::tls(leaf, |seen| {
        Reply::ok(&format!("reached {}", seen.target()))
    })
}

// --- only a declared origin is reached ------------------------------------------

/// **A declared origin is reached**, its certificate verified, its answer
/// read: the control every refusal below is a neighbour of.
#[test]
fn a_declared_origin_is_reached() {
    let server = serving(&ca().server(&[NAME]));
    let answer = granted(server.port()).send(&Request::get(format!(
        "{}/v1/payment_intents/pi_1?expand=x",
        https(NAME, server.port())
    )));
    assert_eq!(
        answer.as_ref().map(|a| (a.status(), a.text())),
        Ok((200, "reached /v1/payment_intents/pi_1?expand=x".to_string()))
    );
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].header("host"),
        Some(format!("{NAME}:{}", server.port()).as_str())
    );
    assert_eq!(seen[0].header("connection"), Some("close"));
}

/// **An origin the grant does not declare is refused before anything is
/// resolved or connected**: another host, another port, another scheme, each
/// served by the same server at the same address.
#[test]
fn an_undeclared_origin_is_never_reached() {
    let server = serving(&ca().server(&[NAME, OTHER]));
    let client = granted(server.port());
    for url in [
        format!("{}/x", https(OTHER, server.port())),
        format!("{}/x", https(NAME, server.port() + 1)),
        format!("http://{NAME}:{}/x", server.port()),
        format!("https://127.0.0.1:{}/x", server.port()),
    ] {
        let refused = client.send(&Request::get(url.clone()));
        assert!(
            matches!(refused, Err(Error::Undeclared { .. })),
            "an undeclared URL is refused"
        );
    }
    assert_eq!(server.connections(), 0);
    let reached = client.send(&Request::get(format!("{}/x", https(NAME, server.port()))));
    assert!(reached.is_ok(), "the declared origin remains reachable");
    assert_eq!(server.connections(), 1);
}

/// **A public origin is https, and an address written in one is global; a
/// development origin's address is this machine's.**
#[test]
fn a_grant_holds_its_origins_to_their_reach() {
    for (grant, refused) in [
        (Grant::none().public("http://api.provider.test"), true),
        (Grant::none().public("https://10.0.0.1"), true),
        (Grant::none().public("https://127.0.0.1:8443"), true),
        (Grant::none().public("https://[::1]"), true),
        (Grant::none().public("https://api.provider.test/v1"), true),
        (Grant::none().development("http://10.0.0.1:3301"), true),
        (Grant::none().development("http://8.8.8.8:3301"), true),
        (Grant::none().public("https://api.stripe.com"), false),
        (Grant::none().public("https://93.184.215.14"), false),
        (Grant::none().development("http://127.0.0.1:3301"), false),
        (Grant::none().development("https://localhost:8443"), false),
    ] {
        assert_eq!(matches!(grant, Err(Error::Config(_))), refused, "{grant:?}");
    }
    let grant = held(
        Grant::none()
            .public("https://API.Stripe.com:443/")
            .and_then(|g| g.development("http://127.0.0.1:3301")),
    );
    assert_eq!(
        grant.origins(),
        vec![
            "https://api.stripe.com".to_string(),
            "http://127.0.0.1:3301".to_string()
        ]
    );
}

// --- every address is checked before any is reached --------------------------------

/// **A public origin whose name is rebound to this machine reaches
/// nothing**: DNS rebinding, refused once the name is resolved and before
/// any connection.
#[test]
fn a_public_origin_rebound_to_loopback_reaches_nothing() {
    let server = serving(&ca().server(&[NAME]));
    let client = client(
        held(Grant::none().public(&https(NAME, server.port()))),
        trusting(),
        &[(NAME, &[LOOPBACK])],
    );
    let refused = client.send(&Request::get(https(NAME, server.port())));
    assert_eq!(
        refused,
        Err(Error::Address {
            origin: https(NAME, server.port()),
            address: LOOPBACK,
            class: "loopback",
        })
    );
    assert_eq!(server.connections(), 0);
}

/// **A public origin whose name resolves to a private, link-local or other
/// special address reaches nothing**, a cloud's metadata service among
/// them, and an IPv4 address mapped into IPv6.
#[test]
fn a_public_origin_reaches_no_special_address() {
    for (address, class) in [
        ("10.0.0.7", "private-use"),
        ("172.16.0.1", "private-use"),
        ("192.168.1.1", "private-use"),
        ("169.254.169.254", "link-local"),
        ("100.64.0.1", "shared address space"),
        ("0.0.0.0", "\"this network\""),
        ("::1", "loopback"),
        ("::ffff:127.0.0.1", "loopback"),
        ("::ffff:10.0.0.7", "private-use"),
        ("fd00::1", "unique-local"),
        ("fe80::1", "link-local"),
        ("224.0.0.1", "multicast"),
    ] {
        let address: IpAddr = address.parse().unwrap();
        let client = client(
            held(Grant::none().public(&https(NAME, 443))),
            trusting(),
            &[(NAME, &[address])],
        );
        let refused = client.send(&Request::get(https(NAME, 443)));
        assert_eq!(
            refused,
            Err(Error::Address {
                origin: format!("https://{NAME}"),
                address,
                class,
            })
        );
    }
}

/// **Every address a name resolves to is checked before any is reached**:
/// one address the origin may not reach refuses them all, though the first
/// would have answered.
#[test]
fn every_address_is_checked_before_any_is_reached() {
    let server = serving(&ca().server(&[NAME]));
    let private: IpAddr = "10.0.0.7".parse().unwrap();
    let client = client(
        held(Grant::none().development(&https(NAME, server.port()))),
        trusting(),
        &[(NAME, &[LOOPBACK, private])],
    );
    let refused = client.send(&Request::get(https(NAME, server.port())));
    assert_eq!(
        refused,
        Err(Error::Address {
            origin: https(NAME, server.port()),
            address: private,
            class: "private-use",
        })
    );
    assert_eq!(server.connections(), 0);
}

/// **A resolver whose answer changes after its first lookup** (a name
/// rebound between a check and a connection), counting its lookups.
struct Rebinding {
    first: IpAddr,
    later: IpAddr,
    lookups: AtomicUsize,
}

impl Resolve for Rebinding {
    fn resolve(&self, _host: &str, _port: u16, _within: Duration) -> Result<Vec<IpAddr>, Error> {
        let before = self.lookups.fetch_add(1, Ordering::SeqCst);
        Ok(vec![if before == 0 { self.first } else { self.later }])
    }
}

/// **A name is looked up once, and the address checked is the address
/// reached** (DNS rebinding proper): a name whose answer changes after its
/// first lookup reaches the server at its first answer, and is looked up
/// once; the server at its later answer is never reached. (Both answers are
/// this machine's, as a development origin's must be: what is held is that
/// nothing is resolved between the check and the connection.)
#[cfg(target_os = "linux")]
#[test]
fn a_name_is_looked_up_once_and_its_checked_answer_reached() {
    let leaf = ca().server(&[NAME]);
    let first = Server::tls(&leaf, |_| Reply::ok("first"));
    let port = first.port();
    let later: IpAddr = "127.0.0.2".parse().unwrap();
    let second = Server::tls_on(SocketAddr::new(later, port), &leaf, |_| Reply::ok("later"));
    let rebinding = Arc::new(Rebinding {
        first: LOOPBACK,
        later,
        lookups: AtomicUsize::new(0),
    });
    let client = held(Client::new(
        held(Grant::none().development(&https(NAME, port))),
        trusting(),
    ))
    .with_resolver(rebinding.clone())
    .with_limits(quick());
    let said = client.send(&Request::get(https(NAME, port)));
    assert_eq!(said.map(|a| a.text()), Ok("first".to_string()));
    assert_eq!(rebinding.lookups.load(Ordering::SeqCst), 1);
    assert_eq!((first.connections(), second.connections()), (1, 0));
}

/// **A development origin reaches this machine and nothing else**: a name
/// resolving to a global address is refused, as one resolving to a private
/// one is.
#[test]
fn a_development_origin_reaches_this_machine_alone() {
    let global: IpAddr = "1.1.1.1".parse().unwrap();
    let client = client(
        held(Grant::none().development(&https(NAME, 9))),
        trusting(),
        &[(NAME, &[global])],
    );
    let refused = client.send(&Request::get(https(NAME, 9)));
    assert!(
        matches!(refused, Err(Error::Address { address, .. }) if address == global),
        "a prohibited DNS answer is refused"
    );
}

/// **An address written as an origin's host is verified as its
/// certificate's address**, as a name is verified as its name.
#[test]
fn an_address_origin_is_verified_by_its_address() {
    let server = serving(&ca().server(&["127.0.0.1"]));
    let origin = format!("https://127.0.0.1:{}", server.port());
    let by_address = client(held(Grant::none().development(&origin)), trusting(), &[]);
    let answer = by_address.send(&Request::get(format!("{origin}/ip")));
    assert_eq!(answer.map(|a| a.text()), Ok("reached /ip".to_string()));
    let named = serving(&ca().server(&[NAME]));
    let origin = format!("https://127.0.0.1:{}", named.port());
    let by_address = client(held(Grant::none().development(&origin)), trusting(), &[]);
    let refused = by_address.send(&Request::get(origin.clone()));
    assert_eq!(
        refused,
        Err(Error::Certificate {
            origin,
            refusal: Refusal::WrongName
        })
    );
}

/// **A name is sent by SNI**: a server holding several sites at one
/// address, which presents a site's certificate only to a client that names
/// it, is reached, and saw the origin's name.
#[test]
fn a_name_is_sent_by_sni() {
    let server = Server::by_name(&ca().server(&[NAME]), |seen| {
        Reply::ok(&format!("reached {}", seen.target()))
    });
    let said = granted(server.port()).send(&Request::get(format!(
        "{}/by-name",
        https(NAME, server.port())
    )));
    assert_eq!(said.map(|a| a.text()), Ok("reached /by-name".to_string()));
    let named: Vec<Option<String>> = server.seen().into_iter().map(|s| s.sni).collect();
    assert_eq!(named, vec![Some(NAME.to_string())]);
}

// --- certificates are verified ------------------------------------------------------

fn refusal_of(leaf: &Leaf, roots: Roots) -> Result<Answer, Error> {
    let server = serving(leaf);
    let client = client(
        held(Grant::none().development(&https(NAME, server.port()))),
        roots,
        &[(NAME, &[LOOPBACK])],
    );
    let said = client.send(&Request::get(https(NAME, server.port())));
    // Whatever was refused, nothing was asked.
    assert!(server.seen().is_empty(), "{:?}", server.seen());
    said
}

fn refused_for(said: Result<Answer, Error>) -> Option<Refusal> {
    match said {
        Err(Error::Certificate { refusal, .. }) => Some(refusal),
        _ => None,
    }
}

/// **An expired certificate is refused**, as such.
#[test]
fn an_expired_certificate_is_refused() {
    let now = testing::now();
    let expired = ca().leaf(&[NAME], now - 10 * DAY_SECONDS, now - DAY_SECONDS);
    assert_eq!(
        refused_for(refusal_of(&expired, trusting())),
        Some(Refusal::Expired)
    );
}

/// **A certificate not yet valid is refused**, as such.
#[test]
fn a_certificate_not_yet_valid_is_refused() {
    let now = testing::now();
    let early = ca().leaf(&[NAME], now + DAY_SECONDS, now + 10 * DAY_SECONDS);
    assert_eq!(
        refused_for(refusal_of(&early, trusting())),
        Some(Refusal::NotValidYet)
    );
}

/// **A certificate for another host is refused**, though the authority is
/// trusted and the certificate current.
#[test]
fn a_certificate_for_another_host_is_refused() {
    let elsewhere = ca().server(&[OTHER]);
    assert_eq!(
        refused_for(refusal_of(&elsewhere, trusting())),
        Some(Refusal::WrongName)
    );
}

/// **A self-signed certificate is refused**, and so is one of an authority
/// the client was not given.
#[test]
fn a_certificate_of_no_given_root_is_refused() {
    assert_eq!(
        refused_for(refusal_of(&testing::self_signed(&[NAME]), trusting())),
        Some(Refusal::UnknownIssuer)
    );
    let stranger = TestCa::new("Another authority");
    assert_eq!(
        refused_for(refusal_of(&stranger.server(&[NAME]), trusting())),
        Some(Refusal::UnknownIssuer)
    );
}

/// **The platform's roots do not trust the test's authority**: a
/// deployment's client refuses what only a test installs.
#[test]
fn the_platforms_roots_do_not_trust_a_tests_authority() {
    let platform = held(Roots::platform());
    assert!(!platform.is_empty());
    assert_eq!(
        refused_for(refusal_of(&ca().server(&[NAME]), platform)),
        Some(Refusal::UnknownIssuer)
    );
    assert_eq!(
        refused_for(refusal_of(&ca().server(&[NAME]), Roots::none())),
        Some(Refusal::UnknownIssuer)
    );
}

// --- no redirect is followed ---------------------------------------------------------

/// **A redirect is refused, never followed**, though it names an origin the
/// grant declares and a server there would answer; what is not a redirect is
/// an answer.
#[test]
fn a_redirect_is_never_followed() {
    let target = serving(&ca().server(&[OTHER]));
    let answered = format!("{}/followed?echoed=sk_test_x", https(OTHER, target.port()));
    let location = https(OTHER, target.port());
    let server = Server::tls(&ca().server(&[NAME]), move |seen| {
        let status = seen.target().trim_start_matches("/s/");
        Reply::Bytes(
            format!("HTTP/1.1 {status} Moved\r\nlocation: {answered}\r\ncontent-length: 0\r\n\r\n")
                .into_bytes(),
        )
    });
    let client = client(
        held(
            Grant::none()
                .development(&https(NAME, server.port()))
                .and_then(|g| g.development(&https(OTHER, target.port()))),
        ),
        trusting(),
        &[(NAME, &[LOOPBACK]), (OTHER, &[LOOPBACK])],
    );
    for status in [301u16, 302, 303, 307, 308] {
        let said = client.send(&Request::post(
            format!("{}/s/{status}", https(NAME, server.port())),
            "application/json",
            b"{}".to_vec(),
        ));
        assert_eq!(
            said,
            Err(Error::Redirected {
                status,
                location: Some(location.clone())
            })
        );
    }
    assert_eq!(target.connections(), 0);
    for status in [300u16, 304] {
        let said = client.send(&Request::get(format!(
            "{}/s/{status}",
            https(NAME, server.port())
        )));
        assert_eq!(said.map(|a| a.status()), Ok(status));
    }
    assert_eq!(target.connections(), 0);
}

// --- one deadline ---------------------------------------------------------------------

fn timed(limits: Limits, port: u16) -> (Result<Answer, Error>, Duration) {
    let client = granted(port).with_limits(limits);
    let started = Instant::now();
    let said = client.send(&Request::get(https(NAME, port)));
    (said, started.elapsed())
}

/// **A server that takes the connection and never begins the handshake is
/// given up at the handshake's bound.**
#[test]
fn a_handshake_never_begun_is_given_up() {
    let server = Server::mute(Duration::from_secs(10));
    let (said, took) = timed(
        Limits {
            handshake: Duration::from_millis(300),
            total: Duration::from_secs(8),
            ..quick()
        },
        server.port(),
    );
    assert_eq!(
        said,
        Err(Error::TimedOut {
            phase: Phase::Handshake
        })
    );
    assert!(took < Duration::from_secs(3), "{took:?}");
}

/// **A server that never answers is given up at the total.**
#[test]
fn an_answer_never_sent_is_given_up() {
    let server = Server::tls(&ca().server(&[NAME]), |_| {
        Reply::Hold(Vec::new(), Duration::from_secs(10))
    });
    let (said, took) = timed(
        Limits {
            total: Duration::from_secs(3),
            ..quick()
        },
        server.port(),
    );
    assert_eq!(
        said,
        Err(Error::TimedOut {
            phase: Phase::Exchange
        })
    );
    assert!(took < Duration::from_secs(6), "{took:?}");
}

/// **A server that drips its answer, each byte well within any single
/// wait, is given up at the total**: the bound is on the answer, not on
/// each read (the development provider's wait bounded each read).
#[test]
fn an_answer_dripped_is_given_up_at_the_total() {
    let mut answer = b"HTTP/1.1 200 OK\r\ncontent-length: 400\r\n\r\n".to_vec();
    answer.extend_from_slice(&[b'a'; 400]);
    let server = Server::tls(&ca().server(&[NAME]), move |_| Reply::Drip {
        bytes: answer.clone(),
        every: Duration::from_millis(50),
    });
    let (said, took) = timed(
        Limits {
            total: Duration::from_secs(3),
            ..quick()
        },
        server.port(),
    );
    assert_eq!(
        said,
        Err(Error::TimedOut {
            phase: Phase::Exchange
        })
    );
    assert!(took < Duration::from_secs(6), "{took:?}");
}

/// **The exchange has what is left of the total, past the handshake's own
/// bound**: an answer that takes longer than the handshake may, and less
/// than the total, is read.
#[test]
fn the_exchange_has_the_rest_of_the_total() {
    let server = Server::tls(&ca().server(&[NAME]), |_| Reply::Drip {
        bytes: b"HTTP/1.1 200 OK\r\ncontent-length: 4\r\n\r\nslow".to_vec(),
        every: Duration::from_millis(75),
    });
    let (said, _) = timed(
        Limits {
            handshake: Duration::from_secs(2),
            total: Duration::from_secs(10),
            ..quick()
        },
        server.port(),
    );
    assert_eq!(said.map(|a| a.text()), Ok("slow".to_string()));
}

/// **An answer held open after its last byte is read by its length**, not
/// waited on: the provider that sends its events before it closes does not
/// hold the host (ADR-0332, step eight).
#[test]
fn an_answer_held_open_is_read_by_its_length() {
    let server = Server::tls(&ca().server(&[NAME]), |_| {
        Reply::Hold(
            b"HTTP/1.1 200 OK\r\ncontent-length: 4\r\n\r\nheld".to_vec(),
            Duration::from_secs(10),
        )
    });
    let (said, took) = timed(quick(), server.port());
    assert_eq!(said.map(|a| a.text()), Ok("held".to_string()));
    assert!(took < Duration::from_secs(5), "{took:?}");
}

/// **No wait is longer than a day**: limits past it are held to it, so no
/// deadline is past what the clock holds, and a client so made still answers.
#[test]
fn no_wait_is_longer_than_a_day() {
    let server = serving(&ca().server(&[NAME]));
    let client = granted(server.port()).with_limits(Limits {
        connect: Duration::MAX,
        handshake: Duration::MAX,
        total: Duration::MAX,
        ..quick()
    });
    let limits = client.limits();
    assert_eq!(
        (limits.connect, limits.handshake, limits.total),
        (MOST_WAIT, MOST_WAIT, MOST_WAIT)
    );
    let said = client.send(&Request::get(https(NAME, server.port())));
    assert_eq!(said.map(|a| a.status()), Ok(200));
}

/// **A write to a peer that never reads is given up at the total**: a
/// request larger than the sockets' buffers, sent to a server that takes the
/// connection and reads nothing, waits only for what is left of the
/// deadline, as every read does (each write's wait is the deadline's).
#[test]
fn a_write_to_a_peer_that_never_reads_is_given_up() {
    let server = Server::mute(Duration::from_secs(8));
    let origin = format!("http://127.0.0.1:{}", server.port());
    let client = held(Client::new(
        held(Grant::none().development(&origin)),
        Roots::none(),
    ))
    .with_limits(Limits {
        total: Duration::from_secs(1),
        ..quick()
    });
    let started = Instant::now();
    let said = client.send(&Request::post(
        format!("{origin}/upload"),
        "application/octet-stream",
        vec![b'a'; 64 * 1024 * 1024],
    ));
    let took = started.elapsed();
    assert_eq!(
        said,
        Err(Error::TimedOut {
            phase: Phase::Exchange
        })
    );
    assert!(took < Duration::from_secs(4), "{took:?}");
}

/// **A read or a write begun past the deadline is the deadline's**: it is
/// refused as timed out, and never handed to the socket, which takes no wait
/// of zero. (Mostly the socket's own wait ends an exchange; this is the case
/// it cannot: a read begun after the last byte that came in time.)
#[test]
fn a_read_or_a_write_past_the_deadline_is_timed_out() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let tcp = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let past = Instant::now()
        .checked_sub(Duration::from_millis(1))
        .unwrap_or_else(Instant::now);
    let mut sock = Sock {
        tcp,
        deadline: past,
    };
    let read = sock.read(&mut [0u8; 8]).map_err(|e| e.kind());
    assert_eq!(read, Err(std::io::ErrorKind::TimedOut));
    let written = sock.write(b"x").map_err(|e| e.kind());
    assert_eq!(written, Err(std::io::ErrorKind::TimedOut));
    drop(listener);
}

/// **A connection the server's host never accepts is given up at the
/// connection's bound**: a listener whose queue is full drops each new
/// connection's first packet, as an address that answers nothing does.
#[cfg(target_os = "linux")]
#[test]
fn a_connection_never_accepted_is_given_up() {
    let (listener, queued, port) = full_listener();
    let (said, took) = timed(
        Limits {
            connect: Duration::from_millis(300),
            total: Duration::from_secs(8),
            ..quick()
        },
        port,
    );
    assert_eq!(
        said,
        Err(Error::TimedOut {
            phase: Phase::Connect
        })
    );
    assert!(took < Duration::from_secs(3), "{took:?}");
    drop((queued, listener));
}

/// **Each address is given its share of the connection's bound**: a name
/// whose first address never takes a connection, and whose second does, is
/// reached through the second within the bound.
#[cfg(target_os = "linux")]
#[test]
fn an_address_that_never_answers_leaves_the_next_its_share() {
    let (listener, queued, port) = full_listener();
    let second: IpAddr = "127.0.0.2".parse().unwrap();
    let server = Server::tls_on(
        std::net::SocketAddr::new(second, port),
        &ca().server(&[NAME]),
        |seen| Reply::ok(&format!("reached {}", seen.target())),
    );
    let client = client(
        held(Grant::none().development(&https(NAME, port))),
        trusting(),
        &[(NAME, &[LOOPBACK, second])],
    )
    .with_limits(Limits {
        connect: Duration::from_secs(1),
        total: Duration::from_secs(8),
        ..quick()
    });
    let started = Instant::now();
    let said = client.send(&Request::get(format!("{}/second", https(NAME, port))));
    let took = started.elapsed();
    assert_eq!(said.map(|a| a.text()), Ok("reached /second".to_string()));
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert_eq!(server.connections(), 1);
    drop((queued, listener));
}

/// A loopback listener with a queue of none (`listen(fd, 0)`), filled by one
/// connection, so that Linux drops the next connection's SYN.
#[cfg(target_os = "linux")]
fn full_listener() -> (std::net::TcpListener, Vec<TcpStream>, u16) {
    use std::os::fd::FromRawFd;
    // SAFETY: a socket made, bound and listened on here alone, and owned by
    // the `TcpListener` made from it.
    let listener = unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        assert!(fd >= 0);
        let mut address: libc::sockaddr_in = std::mem::zeroed();
        address.sin_family = libc::AF_INET as libc::sa_family_t;
        address.sin_addr.s_addr = u32::from(Ipv4Addr::LOCALHOST).to_be();
        let bound = libc::bind(
            fd,
            (&address as *const libc::sockaddr_in).cast(),
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        );
        assert_eq!(bound, 0);
        assert_eq!(libc::listen(fd, 0), 0);
        std::net::TcpListener::from_raw_fd(fd)
    };
    let port = listener.local_addr().unwrap().port();
    let mut queued = Vec::new();
    // Its queue filled: connections until one is not taken in time.
    for _ in 0..8 {
        match TcpStream::connect_timeout(
            &std::net::SocketAddr::new(LOOPBACK, port),
            Duration::from_millis(200),
        ) {
            Ok(c) => queued.push(c),
            Err(_) => break,
        }
    }
    (listener, queued, port)
}

// --- an answer is bounded -------------------------------------------------------------

/// **A stated length past the body's bound is refused before a byte of the
/// body is read**, the connection then held open by the server; one at the
/// bound is read.
#[test]
fn a_length_past_the_bound_is_refused_unread() {
    let server = Server::tls(&ca().server(&[NAME]), |seen| {
        let length = if seen.target() == "/huge" {
            "99999999999999"
        } else {
            "65536"
        };
        let mut answer =
            format!("HTTP/1.1 200 OK\r\ncontent-length: {length}\r\n\r\n").into_bytes();
        if length == "65536" {
            answer.extend_from_slice(&[b'a'; 65536]);
        }
        Reply::Hold(answer, Duration::from_secs(10))
    });
    let client = granted(server.port());
    let started = Instant::now();
    let huge = client.send(&Request::get(format!(
        "{}/huge",
        https(NAME, server.port())
    )));
    assert_eq!(
        huge,
        Err(Error::TooLarge {
            what: "body",
            limit: 64 * 1024
        })
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let at = client.send(&Request::get(format!("{}/at", https(NAME, server.port()))));
    assert_eq!(at.map(|a| a.body().len()), Ok(65536));
}

/// **A head past its bound is refused as it arrives**, a server sending one
/// field forever among them.
#[test]
fn a_head_past_the_bound_is_refused() {
    let server = Server::tls(&ca().server(&[NAME]), |_| {
        let mut endless = b"HTTP/1.1 200 OK\r\nx-endless: ".to_vec();
        endless.extend(std::iter::repeat_n(b'a', 1024 * 1024));
        Reply::Bytes(endless)
    });
    let said = granted(server.port()).send(&Request::get(https(NAME, server.port())));
    assert_eq!(
        said,
        Err(Error::TooLarge {
            what: "head",
            limit: 16 * 1024
        })
    );
}

/// **The framing is held over TLS as over a buffer**: a chunked answer is
/// read, one cut short is refused, and so is one stating both lengths.
#[test]
fn the_framing_is_held_over_tls() {
    let server = Server::tls(&ca().server(&[NAME]), |seen| {
        Reply::Bytes(match seen.target() {
            "/chunked" => b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n".to_vec(),
            "/cut" => b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\nok".to_vec(),
            _ => b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\ntransfer-encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n".to_vec(),
        })
    });
    let client = granted(server.port());
    let get = |path: &str| {
        client.send(&Request::get(format!(
            "{}{path}",
            https(NAME, server.port())
        )))
    };
    assert_eq!(get("/chunked").map(|a| a.text()), Ok("ok".to_string()));
    let cut = get("/cut");
    assert!(
        matches!(cut, Err(Error::Framing(_))),
        "a truncated body is refused"
    );
    let both = get("/both");
    assert!(
        matches!(both, Err(Error::Framing(_))),
        "ambiguous framing is refused"
    );
}

// --- a secret goes to the connection and nowhere else -------------------------------

/// **A secret reaches the server it is sent to, and nothing else**: not the
/// request's `Debug`, nor any error the client says, whatever went wrong.
#[test]
fn a_secret_is_sent_and_said_nowhere() {
    let server = Server::tls(&ca().server(&[NAME]), |seen| match seen.path() {
        "/ok" => Reply::Bytes(
            b"HTTP/1.1 200 OK\r\nset-cookie: session=cookie_secret_6\r\n\
              content-length: 22\r\n\r\nclient_secret_answer_5"
                .to_vec(),
        ),
        "/framing" => Reply::Bytes(b"HTTP/1.1 200 OK\r\n\r\nno length".to_vec()),
        "/redirect" => Reply::Bytes(
            b"HTTP/1.1 307 X\r\nlocation: https://elsewhere.test/\r\ncontent-length: 0\r\n\r\n"
                .to_vec(),
        ),
        _ => Reply::Hold(Vec::new(), Duration::from_secs(10)),
    });
    let client = granted(server.port()).with_limits(Limits {
        total: Duration::from_secs(2),
        ..quick()
    });
    let request = |path: &str| {
        Request::post(
            format!("{}{path}?token=x", https(NAME, server.port())),
            "application/x-www-form-urlencoded",
            b"client_secret=form_secret_7".to_vec(),
        )
        .secret("Authorization", Secret::new(SECRET))
    };
    let sent = client.send(&request("/ok"));
    assert!(sent.is_ok(), "the secret-bearing request succeeds");
    let seen = server.seen();
    assert_eq!(seen[0].header("authorization"), Some(SECRET));
    let alone = format!("{:?}", Secret::new(SECRET));
    assert!(!alone.contains("sk_test"), "{alone}");
    let shown = format!("{:?}", request("/hooks/hook_secret_9"));
    assert!(
        !shown.contains("sk_test")
            && !shown.contains("form_secret")
            && !shown.contains("token")
            && !shown.contains("hook_secret"),
        "the formatted boundary preserves only safe diagnostic context"
    );
    assert!(
        shown.contains("Authorization") && shown.contains(NAME),
        "the formatted boundary preserves only safe diagnostic context"
    );
    // An answer's `Debug`: its fields' names, never their values or its body.
    let answer = format!("{:?}", sent.as_ref().ok());
    assert!(
        !answer.contains("client_secret_answer") && !answer.contains("cookie_secret"),
        "{answer}"
    );
    assert!(answer.contains("set-cookie"), "{answer}");
    let expired = ca().leaf(
        &[NAME],
        testing::now() - 10 * DAY_SECONDS,
        testing::now() - DAY_SECONDS,
    );
    let refusing = Server::tls(&expired, |_| Reply::ok("never"));
    let refused = granted(refusing.port()).send(
        &Request::get(https(NAME, refusing.port())).secret("Authorization", Secret::new(SECRET)),
    );
    let mut errors = vec![refused];
    for path in ["/framing", "/redirect", "/hold"] {
        errors.push(client.send(&request(path)));
    }
    errors.push(client.send(&request("/ok").header("x-split", "a\r\nb")));
    errors.push(
        client.send(
            &Request::get(format!("{}/x", https(OTHER, server.port())))
                .secret("Authorization", Secret::new(SECRET)),
        ),
    );
    assert_eq!(errors.len(), 6);
    let kinds: Vec<&str> = errors
        .iter()
        .map(|said| match said {
            Err(Error::Certificate { .. }) => "certificate",
            Err(Error::Framing(_)) => "framing",
            Err(Error::Redirected { .. }) => "redirected",
            Err(Error::TimedOut { .. }) => "timed out",
            Err(Error::Request(_)) => "request",
            Err(Error::Undeclared { .. }) => "undeclared",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "certificate",
            "framing",
            "redirected",
            "timed out",
            "request",
            "undeclared"
        ]
    );
    for said in errors {
        assert!(said.is_err(), "{said:?}");
        let shown = format!("{said:?} {}", said.clone().err().unwrap());
        assert!(
            !shown.contains("sk_test") && !shown.contains("form_secret"),
            "the formatted boundary preserves only safe diagnostic context"
        );
    }
}

/// **A server that echoes the request has none of it said in the client's
/// error**: the request line it sends back where a status line belongs (the
/// path and the query, and a secret's field after it) is refused without a
/// byte of it quoted.
#[test]
fn an_echoed_request_is_quoted_in_no_error() {
    let server = Server::plain(|seen| Reply::Bytes(seen.raw.clone()));
    let origin = format!("http://127.0.0.1:{}", server.port());
    let client = held(Client::new(
        held(Grant::none().development(&origin)),
        Roots::none(),
    ))
    .with_limits(quick());
    let said = client.send(
        &Request::get(format!("{origin}/hooks/hook_secret_9?token=tok_secret_8"))
            .secret("authorization", Secret::new(SECRET)),
    );
    assert_eq!(
        said,
        Err(Error::Framing(
            "the answer's status line is not one of HTTP/1.1 or HTTP/1.0".into()
        ))
    );
    let seen = server.seen();
    assert_eq!(seen.len(), 1, "the request was sent, and echoed");
    let shown = format!("{said:?} {}", said.clone().err().unwrap());
    assert!(
        !shown.contains("hook_secret")
            && !shown.contains("tok_secret")
            && !shown.contains("sk_test"),
        "the formatted boundary preserves only safe diagnostic context"
    );
}

/// **A field that would end its line is refused before anything is sent**,
/// so no value splits the request into two.
#[test]
fn a_field_that_would_split_the_request_is_never_sent() {
    let server = serving(&ca().server(&[NAME]));
    let client = granted(server.port());
    for request in [
        Request::get(https(NAME, server.port())).header("x-id", "1\r\nx-injected: 1"),
        Request::get(https(NAME, server.port()))
            .secret("authorization", Secret::new("Bearer a\r\nx-injected: 1")),
        Request::get(https(NAME, server.port())).header("Host", "elsewhere.test"),
        Request::get(https(NAME, server.port())).header("Transfer-Encoding", "chunked"),
        Request::get(format!(
            "{}/a b HTTP/1.1\r\nx-injected: 1",
            https(NAME, server.port())
        )),
    ] {
        let refused = client.send(&request);
        assert!(
            matches!(refused, Err(Error::Request(_))),
            "an invalid request is refused"
        );
    }
    assert_eq!(server.connections(), 0);
}
