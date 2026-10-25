//! **A test's certificate authority and servers** (this crate's tests, and a
//! dependent's through the `test-ca` feature): an authority made for the
//! run, the certificates it issues, and servers that answer as a test says.
//! Nothing here is a trust decision: a client trusts the authority only
//! where a test gives its root to [`crate::Roots::only`]. Its keys are made
//! for each run and never written down (charter §3.7: no private
//! certificate material in the repository).
//!
//! The certificates are DER this module writes itself over *ring*, already
//! the TLS provider: an issuer's name, a validity, an ECDSA P-256 key, and
//! the extensions WebPKI reads (basic constraints, key usage, subject
//! alternative names, server authentication).

use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

// --- DER, as much as a certificate needs ---------------------------------------

fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = content.len();
    if n < 0x80 {
        out.push(n as u8);
    } else {
        let bytes = n.to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        out.push(0x80 | (bytes.len() - skip) as u8);
        out.extend_from_slice(&bytes[skip..]);
    }
    out.extend_from_slice(content);
    out
}

fn sequence(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

fn oid(content: &[u8]) -> Vec<u8> {
    tlv(0x06, content)
}

fn integer(magnitude: &[u8]) -> Vec<u8> {
    let mut bytes: Vec<u8> = magnitude.iter().copied().skip_while(|b| *b == 0).collect();
    if bytes.first().is_none_or(|b| b & 0x80 != 0) {
        bytes.insert(0, 0);
    }
    tlv(0x02, &bytes)
}

fn bits(content: &[u8], unused: u8) -> Vec<u8> {
    let mut all = vec![unused];
    all.extend_from_slice(content);
    tlv(0x03, &all)
}

const ECDSA_WITH_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const PRIME256V1: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];
const BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];
const KEY_USAGE: &[u8] = &[0x55, 0x1d, 0x0f];
const SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1d, 0x11];
const EXT_KEY_USAGE: &[u8] = &[0x55, 0x1d, 0x25];
const SERVER_AUTH: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];

fn name(common: &str) -> Vec<u8> {
    sequence(&[tlv(
        0x31,
        &sequence(&[oid(COMMON_NAME), tlv(0x0c, common.as_bytes())]),
    )])
}

fn extension(id: &[u8], critical: bool, value: Vec<u8>) -> Vec<u8> {
    let mut parts = vec![oid(id)];
    if critical {
        parts.push(tlv(0x01, &[0xff]));
    }
    parts.push(tlv(0x04, &value));
    sequence(&parts)
}

/// Seconds since 1970 as UTCTime (to 2049) or GeneralizedTime (RFC 5280
/// §4.1.2.5).
fn time(unix: i64) -> Vec<u8> {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Days to a civil date (H. Hinnant, "chrono-Compatible Low-Level Date
    // Algorithms", days_from_civil's inverse).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let clock = format!(
        "{month:02}{day:02}{:02}{:02}{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    );
    if (1950..2050).contains(&year) {
        tlv(0x17, format!("{:02}{clock}", year % 100).as_bytes())
    } else {
        tlv(0x18, format!("{year:04}{clock}").as_bytes())
    }
}

/// Now, in seconds since 1970.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A day, in seconds.
pub const DAY_SECONDS: i64 = 86_400;

struct Key {
    pair: EcdsaKeyPair,
    pkcs8: Vec<u8>,
}

fn key() -> Key {
    let random = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random)
        .expect("a P-256 key")
        .as_ref()
        .to_vec();
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &pkcs8, &random)
        .expect("the key just made");
    Key { pair, pkcs8 }
}

/// **A certificate**: `subject` with `key`'s public key, valid from `from`
/// to `until`, issued as `issuer` by `signer`, with `extensions`.
fn certificate(
    subject: &[u8],
    key: &Key,
    issuer: &[u8],
    signer: &Key,
    from: i64,
    until: i64,
    extensions: Vec<Vec<u8>>,
) -> Vec<u8> {
    let random = SystemRandom::new();
    let mut serial = [0u8; 16];
    random.fill(&mut serial).expect("a serial number");
    serial[0] = (serial[0] & 0x7f) | 0x01;
    let algorithm = sequence(&[oid(ECDSA_WITH_SHA256)]);
    let public = sequence(&[
        sequence(&[oid(EC_PUBLIC_KEY), oid(PRIME256V1)]),
        bits(key.pair.public_key().as_ref(), 0),
    ]);
    let tbs = sequence(&[
        tlv(0xa0, &integer(&[2])),
        integer(&serial),
        algorithm.clone(),
        issuer.to_vec(),
        sequence(&[time(from), time(until)]),
        subject.to_vec(),
        public,
        tlv(0xa3, &sequence(&extensions)),
    ]);
    let signature = signer.pair.sign(&random, &tbs).expect("signed");
    sequence(&[tbs, algorithm, bits(signature.as_ref(), 0)])
}

/// **A server's certificate and key**: its chain, the leaf first, and its
/// PKCS #8 key.
#[derive(Clone)]
pub struct Leaf {
    pub chain: Vec<Vec<u8>>,
    pub key: Vec<u8>,
}

fn leaf_extensions(names: &[&str]) -> Vec<Vec<u8>> {
    let alternatives: Vec<Vec<u8>> = names
        .iter()
        .map(|n| match n.parse::<IpAddr>() {
            Ok(IpAddr::V4(a)) => tlv(0x87, &a.octets()),
            Ok(IpAddr::V6(a)) => tlv(0x87, &a.octets()),
            Err(_) => tlv(0x82, n.as_bytes()),
        })
        .collect();
    vec![
        extension(BASIC_CONSTRAINTS, true, sequence(&[])),
        extension(KEY_USAGE, true, bits(&[0x80], 7)),
        extension(SUBJECT_ALT_NAME, false, sequence(&alternatives)),
        extension(EXT_KEY_USAGE, false, sequence(&[oid(SERVER_AUTH)])),
    ]
}

/// **A certificate authority made for one run.**
pub struct TestCa {
    key: Key,
    name: Vec<u8>,
    der: Vec<u8>,
}

impl TestCa {
    pub fn new(common_name: &str) -> TestCa {
        let key = key();
        let name = name(common_name);
        let der = certificate(
            &name,
            &key,
            &name,
            &key,
            now() - DAY_SECONDS,
            now() + 365 * DAY_SECONDS,
            vec![
                extension(BASIC_CONSTRAINTS, true, sequence(&[tlv(0x01, &[0xff])])),
                extension(KEY_USAGE, true, bits(&[0x06], 1)),
            ],
        );
        TestCa { key, name, der }
    }

    /// Its root certificate, DER: what a test gives `Roots::only`.
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// **A server certificate for `names`** (DNS names or IP addresses),
    /// valid from `from` to `until` (seconds since 1970).
    pub fn leaf(&self, names: &[&str], from: i64, until: i64) -> Leaf {
        let key = key();
        let der = certificate(
            &name(names.first().copied().unwrap_or("server")),
            &key,
            &self.name,
            &self.key,
            from,
            until,
            leaf_extensions(names),
        );
        Leaf {
            chain: vec![der],
            key: key.pkcs8,
        }
    }

    /// A server certificate for `names`, valid from a day ago for a year.
    pub fn server(&self, names: &[&str]) -> Leaf {
        self.leaf(names, now() - DAY_SECONDS, now() + 365 * DAY_SECONDS)
    }
}

/// **A server certificate for `names` issued by itself**: trusted by no
/// root a test gives.
pub fn self_signed(names: &[&str]) -> Leaf {
    let key = key();
    let subject = name(names.first().copied().unwrap_or("server"));
    let der = certificate(
        &subject,
        &key,
        &subject,
        &key,
        now() - DAY_SECONDS,
        now() + 365 * DAY_SECONDS,
        leaf_extensions(names),
    );
    Leaf {
        chain: vec![der],
        key: key.pkcs8,
    }
}

// --- servers ---------------------------------------------------------------------

/// **A request as a test's server saw it**: its head, its body, and the
/// bytes as they came.
#[derive(Debug, Clone)]
pub struct Seen {
    pub head: String,
    pub body: Vec<u8>,
    pub raw: Vec<u8>,
}

impl Seen {
    /// The target of its request line.
    pub fn target(&self) -> &str {
        self.head.split(' ').nth(1).unwrap_or("")
    }

    /// The path of its target, without the query.
    pub fn path(&self) -> &str {
        self.target().split('?').next().unwrap_or("")
    }

    /// A field of its head, its name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.head.split("\r\n").skip(1).find_map(|line| {
            let (n, v) = line.split_once(':')?;
            n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }
}

/// **What a test's server does once it has read a request.**
pub enum Reply {
    /// Sends these bytes, and closes.
    Bytes(Vec<u8>),
    /// Sends these bytes one at a time, `every` apart.
    Drip { bytes: Vec<u8>, every: Duration },
    /// Sends these bytes, then holds the connection open, saying nothing,
    /// for this long.
    Hold(Vec<u8>, Duration),
    /// Closes at once.
    Close,
}

impl Reply {
    /// `200 OK` with `body` as `text/plain`, its length stated.
    pub fn ok(body: &str) -> Reply {
        Reply::Bytes(
            format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        )
    }
}

type Answerer = Arc<dyn Fn(&Seen) -> Reply + Send + Sync>;

/// **A test's server on a port of its own**, answering each connection on a
/// thread of its own, counting the connections it took and keeping each
/// request it read. Stopped when dropped.
pub struct Server {
    address: SocketAddr,
    connections: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start(handle: impl Fn(TcpStream, &Mutex<Vec<Seen>>) + Send + Sync + 'static) -> Server {
        Server::start_on(SocketAddr::from(([127, 0, 0, 1], 0)), handle)
    }

    fn start_on(
        at: SocketAddr,
        handle: impl Fn(TcpStream, &Mutex<Vec<Seen>>) + Send + Sync + 'static,
    ) -> Server {
        let listener = TcpListener::bind(at).expect("a port");
        let address = listener.local_addr().expect("its address");
        let connections = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = Arc::new(handle);
        let (counted, kept, stopped) = (connections.clone(), seen.clone(), stop.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                counted.fetch_add(1, Ordering::SeqCst);
                let (handle, kept) = (handle.clone(), kept.clone());
                std::thread::spawn(move || handle(stream, &kept));
            }
        });
        Server {
            address,
            connections,
            seen,
            stop,
        }
    }

    /// **A plain HTTP server** answering each request as `answer` says.
    pub fn plain(answer: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Server {
        let answer: Answerer = Arc::new(answer);
        Server::start(move |mut tcp, kept| {
            let _ = tcp.set_read_timeout(Some(Duration::from_secs(10)));
            if let Some(seen) = read_request(&mut tcp) {
                kept.lock().expect("kept").push(seen.clone());
                reply(&mut tcp, answer(&seen));
            }
            let _ = tcp.shutdown(Shutdown::Both);
        })
    }

    /// **A TLS server presenting `leaf`**, answering as `answer` says, its
    /// close said with `close_notify`.
    pub fn tls(leaf: &Leaf, answer: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Server {
        Server::tls_on(SocketAddr::from(([127, 0, 0, 1], 0)), leaf, answer)
    }

    /// **A TLS server at `at`** (another loopback address, a port another
    /// listens on elsewhere), as [`Server::tls`] is.
    pub fn tls_on(
        at: SocketAddr,
        leaf: &Leaf,
        answer: impl Fn(&Seen) -> Reply + Send + Sync + 'static,
    ) -> Server {
        let config = server_config(leaf);
        let answer: Answerer = Arc::new(answer);
        Server::start_on(at, move |tcp, kept| {
            let _ = tcp.set_read_timeout(Some(Duration::from_secs(10)));
            let Ok(conn) = ServerConnection::new(config.clone()) else {
                return;
            };
            let mut tls = StreamOwned::new(conn, tcp);
            if let Some(seen) = read_request(&mut tls) {
                kept.lock().expect("kept").push(seen.clone());
                reply(&mut tls, answer(&seen));
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
            let _ = tls.sock.shutdown(Shutdown::Both);
        })
    }

    /// **A server that takes each connection and says nothing** for `hold`:
    /// one that never begins a handshake.
    pub fn mute(hold: Duration) -> Server {
        Server::start(move |tcp, _| {
            std::thread::sleep(hold);
            drop(tcp);
        })
    }

    /// **A TLS front for the plain HTTP server at `backend`**, as a
    /// deployment's load balancer is: each request read, sent on, and the
    /// backend's answer, read by its stated length, sent back.
    pub fn front(leaf: &Leaf, backend: SocketAddr) -> Server {
        Server::tls(leaf, move |seen| {
            let Ok(mut tcp) = TcpStream::connect(backend) else {
                return Reply::Close;
            };
            let _ = tcp.set_read_timeout(Some(Duration::from_secs(10)));
            if tcp.write_all(&seen.raw).is_err() {
                return Reply::Close;
            }
            match read_request(&mut tcp) {
                Some(answer) => Reply::Bytes(answer.raw),
                None => Reply::Close,
            }
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    /// The connections taken so far.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    /// The requests read so far, in order.
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("seen").clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The listener waits in `accept`: one more connection ends its loop.
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(200));
    }
}

fn server_config(leaf: &Leaf) -> Arc<ServerConfig> {
    let chain = leaf
        .chain
        .iter()
        .map(|c| CertificateDer::from(c.clone()))
        .collect();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf.key.clone()));
    Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("TLS 1.2 and 1.3")
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .expect("the leaf and its key"),
    )
}

/// **A message read off `stream`**: its head to the empty line (64 KiB at
/// most), and as many bytes after it as its `content-length` says.
fn read_request(stream: &mut impl Read) -> Option<Seen> {
    let mut raw = Vec::new();
    let mut byte = [0u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        if raw.len() > 64 * 1024 || stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        raw.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&raw).into_owned();
    let length = head
        .split("\r\n")
        .find_map(|line| {
            let (n, v) = line.split_once(':')?;
            n.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = vec![0; length.min(1024 * 1024)];
    stream.read_exact(&mut body).ok()?;
    raw.extend_from_slice(&body);
    Some(Seen { head, body, raw })
}

fn reply(stream: &mut impl Write, reply: Reply) {
    match reply {
        Reply::Bytes(bytes) => {
            let _ = stream.write_all(&bytes);
            let _ = stream.flush();
        }
        Reply::Drip { bytes, every } => {
            for byte in bytes {
                if stream
                    .write_all(&[byte])
                    .and_then(|()| stream.flush())
                    .is_err()
                {
                    return;
                }
                std::thread::sleep(every);
            }
        }
        Reply::Hold(bytes, hold) => {
            let _ = stream.write_all(&bytes);
            let _ = stream.flush();
            std::thread::sleep(hold);
        }
        Reply::Close => {}
    }
}
