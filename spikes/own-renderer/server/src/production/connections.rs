//! **A production host's connections are bounded** (track `deploy`, the
//! review's finding 1): in what they hold and in how long, so no client can
//! hold the host or end it.
//!
//! - **A request arrives by one deadline** ([`Arriving`]): its whole head and
//!   body within `PW_READ_TIMEOUT` of its connection's start, however its
//!   bytes are spaced. Until the review each read waited at most the bound
//!   and the next began it again: a byte every 1.5 s held a connection past
//!   12 s with `PW_READ_TIMEOUT=2`.
//! - **An answer is written by one deadline** ([`Sending`]): its head and its
//!   body within `PW_WRITE_TIMEOUT`, and every write on the connection within
//!   it ([`prepare`]): a client that reads nothing, or a byte at a time, holds
//!   its thread no longer. There was no write bound.
//! - **At most `PW_MAX_CONNECTIONS` connections are open** (`Gate::try_open`):
//!   one more is answered `503` with `Retry-After` by the accepting thread
//!   ([`turn_away`]), which starts none for it. There was no limit: 2,000
//!   connections that each declared a body and sent none held 2,036 threads.
//! - **A connection the system gives no thread is dropped, and the host
//!   lives** ([`spawn`]): `std::thread::spawn` panics where the system
//!   refuses a thread, and in the accepting thread that panic ended the
//!   process (exit 101 under `prlimit --nproc`), which then refused
//!   everything.
//! - **A failure said once, not once a connection** ([`Said`]): a flood of
//!   refusals is one line every ten seconds, with how many it stands for.
//!
//! A thread a connection is the host's known scaling limit (the ADR's
//! "Not claimed"): these bounds keep it alive at its limit, they do not raise
//! the limit.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// **How long a request may take to arrive, and an answer to be written**,
/// set once, by `main`, for a production deployment: none in development,
/// whose connections are as they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bounds {
    /// A request's whole head and body, from its connection's start.
    pub(crate) read: Duration,
    /// An answer's whole head and body, and any one write on the connection.
    pub(crate) write: Duration,
}

static BOUNDS: OnceLock<Bounds> = OnceLock::new();

/// **This process's connections are bounded so from now on.**
pub(crate) fn install(bounds: Bounds) {
    let _ = BOUNDS.set(bounds);
}

fn bounds() -> Option<Bounds> {
    BOUNDS.get().copied()
}

/// What is left until `until`, or the error a passed deadline is.
fn left(until: Instant, what: &str) -> io::Result<Duration> {
    let left = until.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("{what}: its deadline passed"),
        ));
    }
    Ok(left)
}

/// **Whether an error is a deadline's**: a socket's timeout, which Unix
/// reports as `EAGAIN` (`WouldBlock`), or a deadline that had passed.
pub(crate) fn timed_out(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

/// **A connection made ready for its thread** (production only): every write
/// on it bounded, the stream's frames among them, which no [`Sending`]
/// writes.
pub(crate) fn prepare(stream: &TcpStream) {
    if let Some(b) = bounds() {
        let _ = stream.set_write_timeout(Some(b.write));
    }
}

/// **A request's bytes, as they arrive by one deadline**: each read is given
/// what is left of `PW_READ_TIMEOUT` from when the connection was taken, and
/// none once it has passed. Without bounds (development), the stream's reads
/// as they were.
pub(crate) struct Arriving {
    stream: TcpStream,
    until: Option<Instant>,
}

impl Arriving {
    /// The request on `stream`, whose deadline starts now: as its thread
    /// takes it, which is as it is accepted.
    pub(crate) fn new(stream: TcpStream) -> Arriving {
        Arriving {
            until: bounds().map(|b| Instant::now() + b.read),
            stream,
        }
    }
}

impl Read for Arriving {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(until) = self.until {
            self.stream
                .set_read_timeout(Some(left(until, "the request")?))?;
        }
        self.stream.read(buf)
    }
}

/// **An answer's bytes, written by one deadline**: each write is given what
/// is left of `PW_WRITE_TIMEOUT` from when the answer began, and none once
/// it has passed. Without bounds (development), the stream's writes as they
/// were.
pub(crate) struct Sending<'s> {
    stream: &'s TcpStream,
    until: Option<Instant>,
}

impl<'s> Sending<'s> {
    /// An answer begun now on `stream`.
    pub(crate) fn new(stream: &'s TcpStream) -> Sending<'s> {
        Sending {
            stream,
            until: bounds().map(|b| Instant::now() + b.write),
        }
    }
}

impl Write for Sending<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(until) = self.until {
            self.stream
                .set_write_timeout(Some(left(until, "the answer")?))?;
        }
        let mut stream = self.stream;
        stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut stream = self.stream;
        stream.flush()
    }
}

impl Drop for Sending<'_> {
    /// The connection's own bound again, for anything written after.
    fn drop(&mut self) {
        prepare(self.stream);
    }
}

/// **The answer a host at its limit gives**, from the accepting thread: a
/// `503` that says when to ask again (RFC 9110 §10.2.3's `Retry-After`), with
/// the lines every answer of the host carries (`lines`, HSTS).
pub(crate) fn busy(lines: &str) -> Vec<u8> {
    let body = "the host is at its limit of connections: ask again in a second";
    format!(
        "HTTP/1.1 503 Service Unavailable\r\ncontent-type: text/plain; charset=utf-8\r\n\
         content-length: {}\r\nretry-after: 1\r\ncache-control: no-store\r\n\
         x-content-type-options: nosniff\r\n{lines}connection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// The busy answer of this process, made once.
static BUSY: OnceLock<Vec<u8>> = OnceLock::new();

/// **A connection past the limit, turned away** by the accepting thread,
/// which never waits on it: what the client sent already is read, so the
/// close is no reset; the busy answer is written, as much as the socket
/// takes at once (all of it: 300 bytes); and the connection is closed. No
/// thread is started for it, and nothing about it is held.
pub(crate) fn turn_away(stream: TcpStream, lines: &str) {
    let answer = BUSY.get_or_init(|| busy(lines));
    if stream.set_nonblocking(true).is_ok() {
        let mut sent = [0u8; 4096];
        let _ = (&stream).read(&mut sent);
        let _ = (&stream).write(answer);
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    REFUSED.count(
        "busy",
        "connection(s) turned away at the limit (PW_MAX_CONNECTIONS), each answered 503",
    );
}

/// **A connection's thread, started**, or the system's refusal: never a
/// panic. Where it is refused, `run` is dropped unrun, with the connection
/// and its place among the open ones in it: the connection is closed, and
/// counted no longer.
pub(crate) fn spawn(run: impl FnOnce() + Send + 'static) -> io::Result<()> {
    std::thread::Builder::new().spawn(run).map(|_| ())
}

/// **A connection the system gave no thread**, said: dropped, unanswered.
pub(crate) fn not_spawned(e: &io::Error) {
    NOT_SPAWNED.count(
        "spawn-failed",
        &format!("connection(s) dropped: the system gave no thread for them ({e})"),
    );
}

/// **A connection the system did not give**, said, and waited past: `accept`
/// failing again at once (no descriptor left, `EMFILE`) would otherwise
/// spin the accepting thread. It waits 5 ms, doubled each failure in a row,
/// to a second, as Go's `net/http` does (`Server.Serve`).
pub(crate) fn accept_failed(e: &io::Error, failures: &mut u32) {
    ACCEPT_FAILED.count("accept-failed", &format!("connection(s) not taken: {e}"));
    let wait = Duration::from_millis(5)
        .saturating_mul(1 << (*failures).min(8))
        .min(Duration::from_secs(1));
    *failures = failures.saturating_add(1);
    std::thread::sleep(wait);
}

/// **One kind of failure, said at most once in ten seconds**, with how many
/// it stands for since the line before: a flood of connections refused is a
/// few lines, not one a connection. The first is said at once.
pub(crate) struct Said {
    last: Mutex<Option<Instant>>,
    since: AtomicU64,
}

/// How long one kind of failure's line stands for the ones after it.
const SAID_EVERY: Duration = Duration::from_secs(10);

static REFUSED: Said = Said::new();
static NOT_SPAWNED: Said = Said::new();
static ACCEPT_FAILED: Said = Said::new();

impl Said {
    pub(crate) const fn new() -> Said {
        Said {
            last: Mutex::new(None),
            since: AtomicU64::new(0),
        }
    }

    /// **One more**, said where none was said in the last ten seconds: the
    /// count, and `what` they are. Whether it was said.
    pub(crate) fn count(&self, event: &str, what: &str) -> bool {
        self.since.fetch_add(1, Ordering::SeqCst);
        let Ok(mut last) = self.last.lock() else {
            return false;
        };
        let now = Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < SAID_EVERY) {
            return false;
        }
        *last = Some(now);
        let count = self.since.swap(0, Ordering::SeqCst);
        drop(last);
        super::log::warn(event, &format!("pw dev server: {count} {what}"));
        true
    }
}

/// **How many descriptors a host of `max` connections may need**: three a
/// connection (its socket, its reader's copy, a read's watch), and 256 for
/// its databases, its build and its logs.
pub(crate) fn descriptors_for(max: usize) -> u64 {
    max as u64 * 3 + 256
}

/// **Enough descriptors for `max` connections, or why not**: the soft limit
/// raised to what they need, up to the hard limit, as Go's runtime raises
/// it at start (Go 1.19); a hard limit too low for them is a refusal, since
/// `accept` would fail at a limit below the one the deployment states.
#[cfg(unix)]
pub(crate) fn enough_descriptors(max: usize) -> Result<(), String> {
    // A count of descriptors, as the system's limits are kept (u64 on Linux
    // and macOS).
    let needed: libc::rlim_t = descriptors_for(max);
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a valid rlimit for getrlimit to write.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(format!(
            "the file descriptor limit could not be read: {}",
            io::Error::last_os_error()
        ));
    }
    let enough = |n: libc::rlim_t| n == libc::RLIM_INFINITY || n >= needed;
    if enough(limit.rlim_cur) {
        return Ok(());
    }
    if !enough(limit.rlim_max) {
        return Err(format!(
            "PW_MAX_CONNECTIONS is {max}, which needs {needed} file descriptors, and this \
             process may open {} (its hard limit, RLIMIT_NOFILE): raise it (systemd's \
             LimitNOFILE=, a container's --ulimit nofile=) or state fewer connections",
            limit.rlim_max
        ));
    }
    let raised = libc::rlimit {
        rlim_cur: needed,
        rlim_max: limit.rlim_max,
    };
    // SAFETY: `raised` is a valid rlimit, within the hard limit read above.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } != 0 {
        return Err(format!(
            "the file descriptor limit could not be raised to {needed}: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A connected pair: what a client holds, and what the host accepted.
    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let client = TcpStream::connect(listener.local_addr().expect("address")).expect("connect");
        let (accepted, _) = listener.accept().expect("accept");
        (client, accepted)
    }

    /// **A request's deadline is one for all its reads**: bytes that keep
    /// coming, each well within the bound, do not keep the request's reads
    /// going past it; a request that arrives within it is read whole.
    #[test]
    fn a_requests_deadline_is_one_for_all_its_reads() {
        let (mut client, accepted) = pair();
        let mut arriving = Arriving {
            stream: accepted,
            until: Some(Instant::now() + Duration::from_millis(600)),
        };
        let started = Instant::now();
        let trickle = std::thread::spawn(move || {
            for _ in 0..20 {
                if client.write_all(b"a").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        let mut got = Vec::new();
        let mut byte = [0u8; 1];
        let ended = loop {
            match arriving.read(&mut byte) {
                Ok(0) => break None,
                Ok(_) => got.push(byte[0]),
                Err(e) => break Some(e),
            }
        };
        let took = started.elapsed();
        trickle.join().expect("joined");
        assert!(
            ended.as_ref().is_some_and(timed_out),
            "a request read past its deadline: {ended:?} after {took:?}, {} bytes",
            got.len()
        );
        assert!(took < Duration::from_millis(1500), "{took:?}");
        assert!(got.len() < 8, "{} bytes", got.len());

        // Within its deadline, a request is read whole.
        let (mut client, accepted) = pair();
        let mut arriving = Arriving {
            stream: accepted,
            until: Some(Instant::now() + Duration::from_secs(10)),
        };
        client.write_all(b"GET / HTTP/1.1\r\n\r\n").expect("sent");
        drop(client);
        let mut whole = String::new();
        arriving.read_to_string(&mut whole).expect("read");
        assert_eq!(whole, "GET / HTTP/1.1\r\n\r\n");
    }

    /// **An answer's deadline is one for all its writes**: to a client that
    /// reads nothing, the writing ends at the deadline, cut; to one that
    /// reads, it is written whole.
    #[test]
    fn an_answers_deadline_is_one_for_all_its_writes() {
        let (client, accepted) = pair();
        // The host's buffer kept small, as a full one is: what it writes
        // waits on the client's reads at once.
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let size: libc::c_int = 4096;
            // SAFETY: a valid socket, and an int option of its size.
            unsafe {
                libc::setsockopt(
                    accepted.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&size as *const libc::c_int).cast(),
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
            }
        }
        let (done, ended) = std::sync::mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            let mut sending = Sending {
                stream: &accepted,
                until: Some(Instant::now() + Duration::from_millis(500)),
            };
            let _ = done.send(sending.write_all(&vec![b'x'; 32 << 20]));
        });
        let written = ended.recv_timeout(Duration::from_secs(10));
        let took = started.elapsed();
        assert!(
            matches!(&written, Ok(Err(e)) if timed_out(e)),
            "an answer to a client that reads nothing was written past its deadline: \
             {written:?} after {took:?}"
        );
        assert!(took < Duration::from_secs(3), "{took:?}");
        drop(client);

        let (mut client, accepted) = pair();
        let reader = std::thread::spawn(move || {
            let mut all = Vec::new();
            let _ = client.read_to_end(&mut all);
            all.len()
        });
        {
            let mut sending = Sending {
                stream: &accepted,
                until: Some(Instant::now() + Duration::from_secs(20)),
            };
            sending.write_all(&vec![b'y'; 4 << 20]).expect("written");
        }
        drop(accepted);
        assert_eq!(reader.join().expect("read"), 4 << 20);
    }

    /// **A host at its limit says so, and when to ask again**: 503, a
    /// `Retry-After`, `nosniff`, and the lines every answer carries.
    #[test]
    fn a_host_at_its_limit_says_when_to_ask_again() {
        let answer = String::from_utf8(busy("strict-transport-security: max-age=31536000\r\n"))
            .expect("text");
        let (head, body) = answer.split_once("\r\n\r\n").expect("a head");
        assert!(
            head.starts_with("HTTP/1.1 503 Service Unavailable\r\n"),
            "{head}"
        );
        for line in [
            "retry-after: 1",
            "x-content-type-options: nosniff",
            "strict-transport-security: max-age=31536000",
            "connection: close",
        ] {
            assert!(head.lines().any(|l| l == line), "{line}: {head}");
        }
        assert!(
            head.lines()
                .any(|l| l == format!("content-length: {}", body.len())),
            "{head}"
        );
    }

    /// **A failure is said once in ten seconds**, the first at once.
    #[test]
    fn a_failure_is_said_once_in_ten_seconds() {
        let said = Said::new();
        assert!(said.count("test", "failures"));
        for _ in 0..1000 {
            assert!(!said.count("test", "failures"));
        }
        assert_eq!(said.since.load(Ordering::SeqCst), 1000);
    }

    /// **A thread refused is an error, not a panic**: `run` is dropped, and
    /// what it held with it.
    #[test]
    fn a_thread_started_or_its_refusal() {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn(move || {
            let _ = tx.send(7);
        })
        .expect("a thread");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(7));
    }

    /// **Descriptors for the connections stated**: three a connection, and
    /// what the host needs beside them.
    #[test]
    fn descriptors_for_the_connections_stated() {
        assert_eq!(descriptors_for(1024), 3328);
        assert_eq!(descriptors_for(2), 262);
        #[cfg(unix)]
        assert!(enough_descriptors(2).is_ok());
    }
}
