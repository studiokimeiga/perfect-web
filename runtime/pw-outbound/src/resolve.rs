//! **Names, resolved once.** The client asks a [`Resolve`] for every address
//! a name names, checks each one, and connects to a checked one: nothing is
//! resolved a second time between the check and the connection, so a name
//! rebound in between reaches nothing new.

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use crate::{Error, Phase};

/// **A resolver**: every address `host` names, waited for at most `within`.
pub trait Resolve: Send + Sync {
    fn resolve(&self, host: &str, port: u16, within: Duration) -> Result<Vec<IpAddr>, Error>;
}

/// Lookups one resolver lets run at once: a lookup given up is left to
/// finish on its thread, and a resolver that never answers would otherwise
/// leave a thread for every request.
pub const MOST_LOOKUPS: usize = 32;

/// **The operating system's resolver** (`getaddrinfo`), each lookup on a
/// thread of its own, so that the wait for it is bounded as every other wait
/// is: a lookup still running when the wait ends is left to finish, and its
/// answer to nobody; at most [`MOST_LOOKUPS`] run at once, and one more is
/// refused.
#[derive(Debug, Clone, Default)]
pub struct System {
    running: Arc<AtomicUsize>,
}

impl System {
    pub fn new() -> System {
        System::default()
    }

    /// **`lookup`, waited for at most `within`**, on a thread of its own,
    /// one of at most [`MOST_LOOKUPS`] running.
    pub(crate) fn bounded(
        &self,
        host: &str,
        lookup: impl FnOnce() -> io::Result<Vec<IpAddr>> + Send + 'static,
        within: Duration,
    ) -> Result<Vec<IpAddr>, Error> {
        if self.running.fetch_add(1, Ordering::SeqCst) >= MOST_LOOKUPS {
            self.running.fetch_sub(1, Ordering::SeqCst);
            return Err(Error::Resolve {
                host: host.to_string(),
                why: format!("{MOST_LOOKUPS} lookups are already running"),
            });
        }
        let (sender, answer) = mpsc::channel();
        let running = Running(self.running.clone());
        let spawned = std::thread::Builder::new()
            .name("pw-outbound-resolve".into())
            .spawn(move || {
                let _running = running;
                let _ = sender.send(lookup());
            });
        if let Err(e) = spawned {
            return Err(Error::Resolve {
                host: host.to_string(),
                why: e.to_string(),
            });
        }
        match answer.recv_timeout(within) {
            Ok(Ok(found)) => Ok(found),
            Ok(Err(e)) => Err(Error::Resolve {
                host: host.to_string(),
                why: e.to_string(),
            }),
            Err(RecvTimeoutError::Timeout) => Err(Error::TimedOut {
                phase: Phase::Resolve,
            }),
            Err(RecvTimeoutError::Disconnected) => Err(Error::Resolve {
                host: host.to_string(),
                why: "the lookup ended with no answer".into(),
            }),
        }
    }
}

/// One running lookup, counted until its thread ends, however it ends.
struct Running(Arc<AtomicUsize>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Resolve for System {
    fn resolve(&self, host: &str, port: u16, within: Duration) -> Result<Vec<IpAddr>, Error> {
        let name = host.to_string();
        self.bounded(
            host,
            move || {
                (name.as_str(), port)
                    .to_socket_addrs()
                    .map(|found| found.map(|a| a.ip()).collect())
            },
            within,
        )
    }
}

/// **Names fixed to addresses**, as curl's `--resolve` fixes them: a test's
/// resolver. A name it does not hold names nothing. Every address it gives is
/// checked as the system's are.
#[derive(Debug, Clone, Default)]
pub struct Fixed {
    names: BTreeMap<String, Vec<IpAddr>>,
}

impl Fixed {
    pub fn new() -> Fixed {
        Fixed::default()
    }

    pub fn name(mut self, host: &str, addresses: &[IpAddr]) -> Fixed {
        self.names
            .insert(host.to_ascii_lowercase(), addresses.to_vec());
        self
    }
}

impl Resolve for Fixed {
    fn resolve(&self, host: &str, _port: u16, _within: Duration) -> Result<Vec<IpAddr>, Error> {
        self.names.get(host).cloned().ok_or_else(|| Error::Resolve {
            host: host.to_string(),
            why: "no address is fixed for it".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::time::Instant;

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    /// **A lookup is waited for no longer than it is given**: one that takes
    /// two seconds is given up at a tenth of one; one that answers at once
    /// is taken; and the system's own answers for `localhost`.
    #[test]
    fn a_lookup_is_waited_for_no_longer_than_given() {
        let system = System::new();
        let started = Instant::now();
        let slow = system.bounded(
            "slow.test",
            || {
                std::thread::sleep(Duration::from_secs(2));
                Ok(vec![LOOPBACK])
            },
            Duration::from_millis(100),
        );
        assert_eq!(
            slow,
            Err(Error::TimedOut {
                phase: Phase::Resolve
            })
        );
        assert!(started.elapsed() < Duration::from_millis(1500));
        let quick = system.bounded("quick.test", || Ok(vec![LOOPBACK]), Duration::from_secs(10));
        assert_eq!(quick, Ok(vec![LOOPBACK]));
        let found = system.resolve("localhost", 443, Duration::from_secs(10));
        assert!(
            found
                .as_ref()
                .is_ok_and(|a| !a.is_empty() && a.iter().all(IpAddr::is_loopback)),
            "{found:?}"
        );
    }

    /// **At most [`MOST_LOOKUPS`] lookups run at once**: lookups that never
    /// answer in time fill the resolver, the next is refused at once, and
    /// once they end a lookup is taken again.
    #[test]
    fn lookups_running_at_once_are_bounded() {
        let system = System::new();
        let mut refused_at = None;
        for i in 0..(2 * MOST_LOOKUPS) {
            let said = system.bounded(
                "held.test",
                || {
                    std::thread::sleep(Duration::from_secs(3));
                    Ok(vec![LOOPBACK])
                },
                Duration::from_millis(1),
            );
            if matches!(&said, Err(Error::Resolve { why, .. }) if why.contains("already running")) {
                refused_at = Some(i);
                break;
            }
        }
        assert_eq!(refused_at, Some(MOST_LOOKUPS));
        std::thread::sleep(Duration::from_millis(3500));
        let again = system.bounded("free.test", || Ok(vec![LOOPBACK]), Duration::from_secs(10));
        assert_eq!(again, Ok(vec![LOOPBACK]));
    }
}
