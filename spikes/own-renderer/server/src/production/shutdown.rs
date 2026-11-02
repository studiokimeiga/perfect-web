//! **A host stops when it is told** (track `deploy`): SIGTERM, which
//! systemd, a container's runtime and a platform send to stop a process, or
//! SIGINT. Then the host takes no new connection, says it is not ready, and
//! the connections open are finished within the deployment's grace
//! (`PW_SHUTDOWN_GRACE`, eight seconds where it says nothing: under the ten
//! a container's runtime waits before it kills); the process ends then.
//!
//! Nothing is lost by stopping sooner: a command is one transaction, which a
//! connection cut before its commit rolls back, and a session is kept where
//! the layer keeps it. The grace is for the answers under way.
//!
//! std has no signals, so libc's: the signals are blocked in the main thread
//! before any other starts ([`block`]), so every thread inherits the mask
//! and none but the waiter ([`wait_in_thread`]) receives them, by
//! `sigwait`. A blocked signal is never lost: one sent before the waiter
//! waits is pending, and taken when it does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// **What a host's stop waits on**: whether it is stopping, and how many
/// connections are open.
pub(crate) struct Gate {
    draining: AtomicBool,
    open: Mutex<usize>,
    closed: Condvar,
}

/// The process's gate: `main`'s accept loop and the readiness answer.
pub(crate) static GATE: Gate = Gate::new();

impl Gate {
    pub(crate) const fn new() -> Gate {
        Gate {
            draining: AtomicBool::new(false),
            open: Mutex::new(0),
            closed: Condvar::new(),
        }
    }

    /// Whether the host is stopping.
    pub(crate) fn draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// The host is stopping: no new connection, and not ready.
    pub(crate) fn drain(&self) {
        self.draining.store(true, Ordering::SeqCst);
    }

    /// **A connection open until the guard is dropped.**
    pub(crate) fn open(&self) -> Open<'_> {
        *self.open.lock().expect("open connections") += 1;
        Open(self)
    }

    /// **A connection open until the guard is dropped, where fewer than
    /// `limit` are**; none where `limit` are open already, which the host
    /// turns away (`connections::turn_away`). Counted and checked at once,
    /// so two connections taken together cannot both pass the last place.
    pub(crate) fn try_open(&self, limit: usize) -> Option<Open<'_>> {
        let mut open = self.open.lock().expect("open connections");
        if *open >= limit {
            return None;
        }
        *open += 1;
        Some(Open(self))
    }

    /// **Waits until no connection is open, or `grace` has passed**: how
    /// many were still open.
    pub(crate) fn drained(&self, grace: Duration) -> usize {
        let until = Instant::now() + grace;
        let mut open = self.open.lock().expect("open connections");
        while *open > 0 {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            open = self
                .closed
                .wait_timeout(open, left)
                .expect("open connections")
                .0;
        }
        *open
    }
}

/// **A connection, open while this lives.**
pub(crate) struct Open<'g>(&'g Gate);

impl Drop for Open<'_> {
    fn drop(&mut self) {
        let mut open = self.0.open.lock().expect("open connections");
        *open -= 1;
        if *open == 0 {
            self.0.closed.notify_all();
        }
    }
}

/// **Where the host listens, once it does**: the address a stop wakes its
/// accepting loop at. None while the host starts: a stop then ends the
/// process, whatever its start is doing (the review's finding 5f).
static LISTENING: std::sync::OnceLock<std::net::SocketAddr> = std::sync::OnceLock::new();

/// **The host listens from now on**, and a stop wakes it at `wake`.
pub(crate) fn listening(wake: std::net::SocketAddr) {
    let _ = LISTENING.set(wake);
}

/// **What a stopping signal does**, by how many have come and whether the
/// host listens yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The first, while the host starts (migrating its database, warming its
    /// data): the process ends now. Nothing is lost: a migration is one
    /// transaction, which the closed connection rolls back, its advisory
    /// lock released with it, and a search index is built aside and moved
    /// into place whole.
    Starting,
    /// The first, once it listens: no new connection, and the open ones
    /// finished within the grace.
    Drain(std::net::SocketAddr),
    /// Any after the first: the process ends now, the open connections cut,
    /// as a person who asks twice means (the review's finding 5f).
    AtOnce,
}

/// What the `nth` stopping signal does, where the host listens at
/// `listening`.
pub(crate) fn stop_for(nth: usize, listening: Option<std::net::SocketAddr>) -> Stop {
    match (nth, listening) {
        (1, None) => Stop::Starting,
        (1, Some(wake)) => Stop::Drain(wake),
        _ => Stop::AtOnce,
    }
}

/// **A stopping signal, acted on**: the `nth` to come, `signal` by name.
pub(crate) fn stop(signal: &'static str, nth: usize) {
    match stop_for(nth, LISTENING.get().copied()) {
        Stop::Starting => {
            super::log::info(
                "stopped",
                &format!("pw dev server: {signal} while it started: stopped before it listened"),
            );
            end(0);
        }
        Stop::Drain(wake) => {
            super::log::info(
                "stopping",
                &format!("pw dev server: {signal}: no new connection; the open ones finish first"),
            );
            GATE.drain();
            let _ = std::net::TcpStream::connect_timeout(&wake, Duration::from_secs(1));
        }
        Stop::AtOnce => {
            let open = *GATE.open.lock().unwrap_or_else(|e| e.into_inner());
            super::log::info(
                "stopped",
                &format!(
                    "pw dev server: {signal} again: stopped at once; {open} connection(s) still \
                     open were cut"
                ),
            );
            end(0);
        }
    }
}

/// The process ended with `code`, what it said first written.
fn end(code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stderr().flush();
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}

/// **The address a stop connects to, to wake the accept loop**: the one the
/// host listens on, or its family's loopback where it listens on every
/// address.
pub(crate) fn waking_address(bind: std::net::IpAddr, port: u16) -> std::net::SocketAddr {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    let at = match bind {
        IpAddr::V4(a) if a.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(a) if a.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        other => other,
    };
    std::net::SocketAddr::new(at, port)
}

/// **SIGTERM and SIGINT blocked in this thread**, so that every thread it
/// starts after inherits the mask. `main` calls it first.
#[cfg(unix)]
pub(crate) fn block() -> Result<(), String> {
    let set = stopping_signals()?;
    // SAFETY: `set` is an initialized signal set; the old mask is not asked.
    let failed = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
    if failed != 0 {
        return Err(format!(
            "the stopping signals could not be blocked ({failed})"
        ));
    }
    Ok(())
}

/// The signals a host stops on.
#[cfg(unix)]
fn stopping_signals() -> Result<libc::sigset_t, String> {
    // SAFETY: a sigset_t is plain data, initialized by sigemptyset before
    // any use, and each call is given a pointer to it alone.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        if libc::sigemptyset(&mut set) != 0
            || libc::sigaddset(&mut set, libc::SIGTERM) != 0
            || libc::sigaddset(&mut set, libc::SIGINT) != 0
        {
            return Err("the stopping signals could not be named".to_string());
        }
        Ok(set)
    }
}

/// **The stopping signals, waited for in a thread of its own**, each as it
/// comes: `then` is called with its name and how many have come, the first
/// being 1. Only a thread whose mask blocks them waits: [`block`] first.
#[cfg(unix)]
pub(crate) fn wait_in_thread(
    then: impl Fn(&'static str, usize) + Send + 'static,
) -> Result<(), String> {
    let set = stopping_signals()?;
    std::thread::Builder::new()
        .name("pw-stop".into())
        .spawn(move || {
            for nth in 1.. {
                let mut signal: libc::c_int = 0;
                // SAFETY: `set` is initialized, `signal` is written by sigwait.
                let failed = unsafe { libc::sigwait(&set, &mut signal) };
                let name = match (failed, signal) {
                    (0, libc::SIGTERM) => "SIGTERM",
                    (0, libc::SIGINT) => "SIGINT",
                    // A wait that failed stops the host too: a signal blocked
                    // with no one waiting would be a host nothing can stop.
                    _ => "a failed wait for a signal",
                };
                then(name, nth);
            }
        })
        .map(|_| ())
        .map_err(|e| format!("the thread that waits for a stop: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A gate drains when its last connection closes**: the wait ends
    /// then, not at its grace.
    #[test]
    fn a_gate_drains_when_its_last_connection_closes() {
        let gate = Gate::new();
        assert!(!gate.draining());
        let started = Instant::now();
        std::thread::scope(|scope| {
            let first = gate.open();
            let second = gate.open();
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                drop(first);
                std::thread::sleep(Duration::from_millis(100));
                drop(second);
            });
            gate.drain();
            assert!(gate.draining());
            assert_eq!(gate.drained(Duration::from_secs(20)), 0);
        });
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(200), "{took:?}");
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    /// **A connection still open at the grace is left**: the wait ends at
    /// the grace, and says how many were open.
    #[test]
    fn a_connection_open_at_the_grace_is_left() {
        let gate = Gate::new();
        let _held = gate.open();
        let started = Instant::now();
        assert_eq!(gate.drained(Duration::from_millis(150)), 1);
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(150), "{took:?}");
        assert!(took < Duration::from_secs(5), "{took:?}");
    }

    /// **A stop wakes the loop where it listens**: at its address, or at
    /// its family's loopback where it listens on every one.
    #[test]
    fn a_stop_wakes_the_loop_where_it_listens() {
        use std::net::IpAddr;
        let at =
            |bind: &str| waking_address(bind.parse::<IpAddr>().expect("an address"), 9).to_string();
        assert_eq!(at("0.0.0.0"), "127.0.0.1:9");
        assert_eq!(at("::"), "[::1]:9");
        assert_eq!(at("127.0.0.1"), "127.0.0.1:9");
        assert_eq!(at("10.0.0.7"), "10.0.0.7:9");
    }

    /// **What a stop does, by how many came**: the first while the host
    /// starts ends it; the first once it listens drains it; any after the
    /// first ends it at once.
    #[test]
    fn what_a_stop_does_by_how_many_came() {
        let at: std::net::SocketAddr = "127.0.0.1:9".parse().expect("an address");
        assert_eq!(stop_for(1, None), Stop::Starting);
        assert_eq!(stop_for(1, Some(at)), Stop::Drain(at));
        assert_eq!(stop_for(2, Some(at)), Stop::AtOnce);
        assert_eq!(stop_for(2, None), Stop::AtOnce);
        assert_eq!(stop_for(5, Some(at)), Stop::AtOnce);
    }

    /// **A gate takes connections up to its limit and no more**, and one
    /// again once one closes; each counted while it is open.
    #[test]
    fn a_gate_takes_connections_up_to_its_limit() {
        let gate = Gate::new();
        let first = gate.try_open(2);
        let second = gate.try_open(2);
        assert!(first.is_some() && second.is_some());
        assert!(
            gate.try_open(2).is_none(),
            "a third was taken at a limit of two"
        );
        drop(first);
        let third = gate.try_open(2);
        assert!(third.is_some(), "no place again once one closed");
        assert_eq!(gate.drained(Duration::from_millis(10)), 2);
        drop((second, third));
        assert_eq!(gate.drained(Duration::from_millis(10)), 0);
    }

    /// **A gate with nothing open has drained at once.**
    #[test]
    fn a_gate_with_nothing_open_has_drained() {
        let gate = Gate::new();
        let started = Instant::now();
        assert_eq!(gate.drained(Duration::from_secs(30)), 0);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
