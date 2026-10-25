//! **The host's one outbound client** (track `outbound-https`, ADR-0387).
//!
//! Every platform service that calls another origin (a payment provider, an
//! identity provider's token endpoint, a push service, a verification
//! provider, a webhook's receiver) calls it through a [`Client`], and nothing
//! else in the host opens a connection to another origin. What a client holds,
//! by construction:
//!
//! - **Only a declared origin is reached.** A client is made from a [`Grant`]:
//!   the origins the deployment declares for one service, each a scheme, a
//!   host and a port, compared exactly. A request to any other origin is
//!   refused before anything is resolved or connected.
//! - **A name is resolved once, and every address it names is checked before
//!   any is reached**: a public origin's must each be an ordinary global
//!   unicast address (no address in an IANA special-purpose block, no
//!   multicast), so a name rebound to a loopback, private or link-local
//!   address (a cloud's metadata service among them) reaches nothing. Only an
//!   origin the deployment names as a development one reaches this machine,
//!   and nothing else.
//! - **A certificate is verified** against the [`Roots`] the client is given:
//!   the platform's store in a deployment, a test's own authority in a test.
//!   An expired, not yet valid, wrongly named or self-signed certificate is
//!   refused, said as such.
//! - **No redirect is followed**: a redirect is the [`Error::Redirected`] it
//!   is, so no answer leads the client, or a secret it sent, to another
//!   origin.
//! - **One deadline bounds the whole exchange**, the connection and the
//!   handshake each within their own bound inside it, and every read and
//!   write on the socket waits only for what is left of it. An answer's head
//!   and body are each bounded in bytes, and a stated length past the bound
//!   is refused before a byte of it is read.
//! - **A secret is written to the connection and nowhere else**: a
//!   [`Secret`] header's value is never shown by any `Debug`, any error or
//!   any log (the client logs nothing), and no code outside this crate can
//!   read it back.
//! - **HTTP/1.1 framing is held strictly** (RFC 9112): one request per
//!   connection; an answer is read by its stated length or its chunks, never
//!   to the connection's close; both lengths at once, a length stated twice,
//!   a transfer coding other than `chunked`, a body cut short, bytes past the
//!   end, a folded or malformed field line, and a bare CR or LF are each
//!   refused.
//!
//! TLS is rustls with the *ring* provider; the HTTP/1.1 is this crate's own,
//! over the rustls stream (the smallest dependency set that does the job:
//! ADR-0387's alternatives).

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod address;
mod http1;
mod origin;
mod resolve;
mod secret;
mod tls;

#[cfg(test)]
mod tests;

pub use address::{Class, class};
pub use origin::{Host, Origin, Scheme};
pub use resolve::{Fixed, Resolve, System};
pub use secret::Secret;
pub use tls::Roots;

// --- what can go wrong --------------------------------------------------------

/// **Where an exchange was when its time ran out.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Resolving the origin's name.
    Resolve,
    /// Connecting to one of its checked addresses.
    Connect,
    /// The TLS handshake.
    Handshake,
    /// Sending the request and reading its answer.
    Exchange,
}

/// **Why a server's certificate was refused.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Past its `notAfter`.
    Expired,
    /// Before its `notBefore`.
    NotValidYet,
    /// Issued for another name than the origin's host.
    WrongName,
    /// Issued by no root the client holds: a self-signed certificate, or one
    /// of an authority the client was not given.
    UnknownIssuer,
    /// Any other reason the verifier gave.
    Other(String),
}

/// **Why a request was not answered.** No variant holds a request's header
/// values, its body or its query: an error may be shown and logged, and a
/// secret in it would be one leaked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The request is not one the client writes: a URL it does not read, a
    /// header's name or value, a target holding what a target may not.
    Request(String),
    /// A grant's origin, or the roots, could not be made.
    Config(String),
    /// The origin is not one the client's grant declares.
    Undeclared { origin: String },
    /// The origin's name named no address.
    Resolve { host: String, why: String },
    /// An address the origin names is not one its reach allows: a public
    /// origin's loopback, private or link-local address, or a development
    /// origin's address off this machine.
    Address {
        origin: String,
        address: IpAddr,
        class: &'static str,
    },
    /// No checked address could be connected to.
    Connect { origin: String, why: String },
    /// The deadline passed.
    TimedOut { phase: Phase },
    /// The server's certificate was refused.
    Certificate { origin: String, refusal: Refusal },
    /// TLS failed otherwise.
    Tls { origin: String, why: String },
    /// The answer is not framed as HTTP/1.1 frames one.
    Framing(String),
    /// The answer's head or body is larger than the client takes.
    TooLarge { what: &'static str, limit: usize },
    /// The answer redirects, which the client never follows; `location`
    /// is the origin it points to (none for a relative or unread one), never
    /// its path or query, which may echo the request's.
    Redirected {
        status: u16,
        location: Option<String>,
    },
    /// The connection failed during the exchange.
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Request(why) => write!(f, "the request is not sent: {why}"),
            Error::Config(why) => write!(f, "{why}"),
            Error::Undeclared { origin } => {
                write!(
                    f,
                    "`{origin}` is not an origin this client's grant declares"
                )
            }
            Error::Resolve { host, why } => write!(f, "`{host}` was not resolved: {why}"),
            Error::Address {
                origin,
                address,
                class,
            } => write!(
                f,
                "`{origin}` names {address}, a {class} address, which it may not reach"
            ),
            Error::Connect { origin, why } => write!(f, "`{origin}` was not reached: {why}"),
            Error::TimedOut { phase } => write!(f, "no answer within the deadline ({phase:?})"),
            Error::Certificate { origin, refusal } => {
                write!(f, "`{origin}`'s certificate is refused: {refusal:?}")
            }
            Error::Tls { origin, why } => write!(f, "TLS with `{origin}` failed: {why}"),
            Error::Framing(why) => write!(f, "the answer is not read: {why}"),
            Error::TooLarge { what, limit } => {
                write!(f, "the answer's {what} is larger than {limit} bytes")
            }
            Error::Redirected { status, location } => write!(
                f,
                "the answer redirects ({status}, to {}), and no redirect is followed",
                location.as_deref().unwrap_or("nowhere said")
            ),
            Error::Io(why) => write!(f, "the connection failed: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// An I/O error, as the phase it came in says it: a socket's wait running out
/// is the deadline's; a TLS error mid-exchange is TLS's.
pub(crate) fn io_error(e: io::Error, phase: Phase) -> Error {
    if matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    ) {
        return Error::TimedOut { phase };
    }
    if let Some(tls) = e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>()) {
        return Error::Io(format!("TLS: {tls}"));
    }
    Error::Io(e.to_string())
}

// --- the bounds -----------------------------------------------------------------

/// **How long, and how much, a client waits for and takes.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// To connect, once the name is resolved.
    pub connect: Duration,
    /// To complete the TLS handshake.
    pub handshake: Duration,
    /// The whole exchange, from the request's start to its answer's last
    /// byte: every phase is within it.
    pub total: Duration,
    /// The answer's head, its interim answers' heads among it, in bytes.
    pub head: usize,
    /// The answer's body, in bytes; a chunked body's framing is held to as
    /// many again.
    pub body: usize,
}

/// The longest any wait is: a day. A longer one is taken as a day, so that
/// no deadline is past what an `Instant` holds.
pub const MOST_WAIT: Duration = Duration::from_secs(24 * 60 * 60);

impl Limits {
    /// These limits, each wait at most [`MOST_WAIT`].
    fn held(self) -> Limits {
        Limits {
            connect: self.connect.min(MOST_WAIT),
            handshake: self.handshake.min(MOST_WAIT),
            total: self.total.min(MOST_WAIT),
            ..self
        }
    }
}

impl Default for Limits {
    /// Five seconds to connect and five to shake hands, ten in all; a head of
    /// 64 KiB and a body of 1 MiB, more than any provider's answer this host
    /// reads.
    fn default() -> Limits {
        Limits {
            connect: Duration::from_secs(5),
            handshake: Duration::from_secs(5),
            total: Duration::from_secs(10),
            head: 64 * 1024,
            body: 1024 * 1024,
        }
    }
}

// --- the grant ----------------------------------------------------------------

/// **Where a declared origin's addresses may be.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The public internet: https, and every address global unicast.
    Public,
    /// This machine: a development origin, every address a loopback one.
    Loopback,
}

impl Reach {
    /// Whether an address is one this reach may connect to; the address's
    /// class where it is not.
    fn admits(self, address: IpAddr) -> Result<(), &'static str> {
        match (self, class(address)) {
            (Reach::Public, Class::Global) => Ok(()),
            (Reach::Loopback, Class::Loopback) => Ok(()),
            (Reach::Loopback, Class::Global) => {
                Err("global (a development origin is this machine's)")
            }
            (_, other) => Err(other.name()),
        }
    }
}

/// **The origins a client may reach**: what the deployment declares for one
/// service, and nothing more. A client is made from a grant, and no request
/// it sends reaches an origin the grant does not hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grant {
    declared: Vec<(Origin, Reach)>,
}

impl Grant {
    /// A grant of nothing: every request refused.
    pub fn none() -> Grant {
        Grant::default()
    }

    /// **An origin on the public internet**: `https://host[:port]`, reached
    /// over TLS, its every address global unicast.
    pub fn public(mut self, origin: &str) -> Result<Grant, Error> {
        let origin = Origin::parse(origin)?;
        if origin.scheme() != Scheme::Https {
            return Err(Error::Config(format!(
                "`{origin}`: a public origin is reached over https alone"
            )));
        }
        if let Host::Ip(address) = origin.host() {
            Reach::Public.admits(*address).map_err(|class| {
                Error::Config(format!(
                    "`{origin}` is a {class} address, which a public origin is not"
                ))
            })?;
        }
        self.declared.push((origin, Reach::Public));
        Ok(self)
    }

    /// **A development origin, on this machine**: http or https, its every
    /// address a loopback one. Only a development deployment names one (the
    /// development payment provider is one), and the host never makes one of
    /// a public origin.
    pub fn development(mut self, origin: &str) -> Result<Grant, Error> {
        let origin = Origin::parse(origin)?;
        if let Host::Ip(address) = origin.host() {
            Reach::Loopback.admits(*address).map_err(|class| {
                Error::Config(format!(
                    "`{origin}` is a {class} address, which a development origin is not"
                ))
            })?;
        }
        self.declared.push((origin, Reach::Loopback));
        Ok(self)
    }

    /// The origins declared, as each is written canonically.
    pub fn origins(&self) -> Vec<String> {
        self.declared.iter().map(|(o, _)| o.to_string()).collect()
    }

    /// The reach of `origin`, where the grant declares it: compared whole,
    /// its scheme, its host and its port.
    fn reach(&self, origin: &Origin) -> Option<Reach> {
        self.declared
            .iter()
            .find(|(declared, _)| declared == origin)
            .map(|(_, reach)| *reach)
    }
}

// --- a request and its answer ---------------------------------------------------

/// **A request's method.** One request per connection; no method the host's
/// services do not use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// **A request**: a method, a URL, headers, secret headers and a body. Its
/// `Debug` shows the origin of its URL, its headers, the names of its
/// secrets and its body's length: no secret, no path or query (a webhook's
/// address may be a secret), and no body, which may hold one (a token
/// exchange's form).
#[derive(Clone)]
pub struct Request {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    secrets: Vec<(String, Secret)>,
    body: Option<Vec<u8>>,
}

impl Request {
    fn new(method: Method, url: impl Into<String>, body: Option<Vec<u8>>) -> Request {
        Request {
            method,
            url: url.into(),
            headers: Vec::new(),
            secrets: Vec::new(),
            body,
        }
    }

    pub fn get(url: impl Into<String>) -> Request {
        Request::new(Method::Get, url, None)
    }

    pub fn delete(url: impl Into<String>) -> Request {
        Request::new(Method::Delete, url, None)
    }

    pub fn post(url: impl Into<String>, content_type: &str, body: impl Into<Vec<u8>>) -> Request {
        Request::new(Method::Post, url, Some(body.into())).header("content-type", content_type)
    }

    pub fn put(url: impl Into<String>, content_type: &str, body: impl Into<Vec<u8>>) -> Request {
        Request::new(Method::Put, url, Some(body.into())).header("content-type", content_type)
    }

    pub fn patch(url: impl Into<String>, content_type: &str, body: impl Into<Vec<u8>>) -> Request {
        Request::new(Method::Patch, url, Some(body.into())).header("content-type", content_type)
    }

    /// A header, written as given: its name a token, its value visible ASCII
    /// and spaces, checked when the request is sent.
    pub fn header(mut self, name: &str, value: &str) -> Request {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// A header whose value is a secret (an `Authorization`, a key): written
    /// to the connection, and shown nowhere.
    pub fn secret(mut self, name: &str, value: Secret) -> Request {
        self.secrets.push((name.to_string(), value));
        self
    }

    pub fn method(&self) -> Method {
        self.method
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let origin = origin::origin_of(&self.url);
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("origin", &origin)
            .field("headers", &self.headers)
            .field(
                "secrets",
                &self.secrets.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            )
            .field("body", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

/// **An answer**: its status, its fields (names lowercase, in order) and its
/// body. Its `Debug` shows its status, its fields' names and its body's
/// length: a provider's answer may hold a secret (a payment's client secret,
/// an identity provider's tokens).
#[derive(Clone, PartialEq, Eq)]
pub struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl fmt::Debug for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Answer")
            .field("status", &self.status)
            .field(
                "headers",
                &self.headers.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            )
            .field("body", &self.body.len())
            .finish()
    }
}

impl Answer {
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The first field of this name, its name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }

    /// The body as text, any byte that is not UTF-8 replaced.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

// --- the client -------------------------------------------------------------------

/// **The host's outbound client, for one service**: its grant, its roots, its
/// resolver and its limits. Cheap to clone; clones share the TLS sessions
/// kept for resumption.
#[derive(Clone)]
pub struct Client {
    grant: Arc<Grant>,
    tls: Arc<rustls::ClientConfig>,
    resolver: Arc<dyn Resolve>,
    limits: Limits,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("grant", &self.grant.origins())
            .field("limits", &self.limits)
            .finish()
    }
}

impl Client {
    /// A client reaching `grant`'s origins alone, verifying their
    /// certificates against `roots`, resolving names with the operating
    /// system's resolver, within the default [`Limits`].
    pub fn new(grant: Grant, roots: Roots) -> Result<Client, Error> {
        Ok(Client {
            grant: Arc::new(grant),
            tls: tls::config(&roots)?,
            resolver: Arc::new(System::new()),
            limits: Limits::default(),
        })
    }

    /// Within `limits`, each wait at most [`MOST_WAIT`].
    pub fn with_limits(mut self, limits: Limits) -> Client {
        self.limits = limits.held();
        self
    }

    /// Names resolved by `resolver` instead: every address it gives is
    /// checked as the system's would be.
    pub fn with_resolver(mut self, resolver: Arc<dyn Resolve>) -> Client {
        self.resolver = resolver;
        self
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    /// **`request`, sent, and its answer read**, or why not. The origin is
    /// checked against the grant before anything else is done.
    pub fn send(&self, request: &Request) -> Result<Answer, Error> {
        let deadline = Instant::now() + self.limits.total;
        let (origin, target) = Origin::split_url(&request.url)?;
        let reach = self.grant.reach(&origin).ok_or_else(|| Error::Undeclared {
            origin: origin.to_string(),
        })?;
        let mut out = http1::request_head(request, &origin, &target)?;
        if let Some(body) = &request.body {
            out.extend_from_slice(body);
        }
        let addresses = self.addresses(&origin, reach, deadline)?;
        let tcp = connect(
            &origin,
            &addresses,
            deadline.min(Instant::now() + self.limits.connect),
        )?;
        let mut sock = Sock { tcp, deadline };
        match origin.scheme() {
            Scheme::Http => exchange(&mut sock, &out, &self.limits),
            Scheme::Https => {
                sock.deadline = deadline.min(Instant::now() + self.limits.handshake);
                let mut conn = tls::handshake(&self.tls, &origin, &mut sock)?;
                sock.deadline = deadline;
                let answer = exchange(
                    &mut rustls::Stream::new(&mut conn, &mut sock),
                    &out,
                    &self.limits,
                );
                // Said once the answer is read; nothing waits on it.
                conn.send_close_notify();
                let _ = conn.write_tls(&mut sock);
                answer
            }
        }
    }

    /// **The addresses `origin` names, each one checked**: resolved once,
    /// within what is left of the deadline, and refused whole where any
    /// address is one its reach may not connect to.
    fn addresses(
        &self,
        origin: &Origin,
        reach: Reach,
        deadline: Instant,
    ) -> Result<Vec<SocketAddr>, Error> {
        let named = match origin.host() {
            Host::Ip(address) => vec![*address],
            Host::Name(name) => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(Error::TimedOut {
                        phase: Phase::Resolve,
                    });
                }
                self.resolver.resolve(name, origin.port(), left)?
            }
        };
        if named.is_empty() {
            return Err(Error::Resolve {
                host: origin.host().to_string(),
                why: "it names no address".to_string(),
            });
        }
        for address in &named {
            reach.admits(*address).map_err(|class| Error::Address {
                origin: origin.to_string(),
                address: *address,
                class,
            })?;
        }
        Ok(named
            .into_iter()
            .map(|address| SocketAddr::new(address, origin.port()))
            .collect())
    }
}

/// **A connection to the first of `addresses` that takes one** by `by`,
/// each address given its share of what is left, so that one that never
/// answers (an unrouted IPv6 address before a working IPv4 one) does not
/// take the others' time.
fn connect(origin: &Origin, addresses: &[SocketAddr], by: Instant) -> Result<TcpStream, Error> {
    let mut last: Option<io::Error> = None;
    for (tried, address) in addresses.iter().enumerate() {
        let left = by.saturating_duration_since(Instant::now());
        let share = left / (addresses.len() - tried) as u32;
        if share.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(address, share) {
            Ok(tcp) => {
                // The request is written whole, at once.
                let _ = tcp.set_nodelay(true);
                return Ok(tcp);
            }
            Err(e) => last = Some(e),
        }
    }
    match last {
        Some(e)
            if !matches!(
                e.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) =>
        {
            Err(Error::Connect {
                origin: origin.to_string(),
                why: e.to_string(),
            })
        }
        _ => Err(Error::TimedOut {
            phase: Phase::Connect,
        }),
    }
}

/// **The request written, and its answer read**, over a plain or a TLS
/// stream.
fn exchange<S: Read + Write>(stream: &mut S, out: &[u8], limits: &Limits) -> Result<Answer, Error> {
    stream
        .write_all(out)
        .and_then(|()| stream.flush())
        .map_err(|e| io_error(e, Phase::Exchange))?;
    http1::read_answer(stream, limits)
}

/// **A socket every read and write of which waits only for what is left of
/// its deadline**, so no peer, however it drips, holds an exchange past it.
pub(crate) struct Sock {
    tcp: TcpStream,
    deadline: Instant,
}

impl Sock {
    fn left(&self) -> io::Result<Duration> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(left)
    }
}

impl Read for Sock {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.left()?;
        self.tcp.set_read_timeout(Some(left))?;
        self.tcp.read(buf)
    }
}

impl Write for Sock {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let left = self.left()?;
        self.tcp.set_write_timeout(Some(left))?;
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}
