//! **Names, resolved once.** The client asks a [`Resolve`] for every address
//! a name names, checks each one, and connects to a checked one: nothing is
//! resolved a second time between the check and the connection, so a name
//! rebound in between reaches nothing new.

use std::collections::BTreeMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use crate::{Error, Phase};

/// **A resolver**: every address `host` names, waited for at most `within`.
pub trait Resolve: Send + Sync {
    fn resolve(&self, host: &str, port: u16, within: Duration) -> Result<Vec<IpAddr>, Error>;
}

/// **The operating system's resolver** (`getaddrinfo`), on a thread of its
/// own, so that the wait for it is bounded as every other wait is: a lookup
/// still running when the wait ends is left to finish, and its answer to
/// nobody.
#[derive(Debug, Clone, Copy, Default)]
pub struct System;

impl Resolve for System {
    fn resolve(&self, host: &str, port: u16, within: Duration) -> Result<Vec<IpAddr>, Error> {
        let (sender, answer) = mpsc::channel();
        let name = host.to_string();
        std::thread::Builder::new()
            .name("pw-outbound-resolve".into())
            .spawn(move || {
                let found = (name.as_str(), port)
                    .to_socket_addrs()
                    .map(|found| found.map(|a| a.ip()).collect::<Vec<_>>());
                let _ = sender.send(found);
            })
            .map_err(|e| Error::Resolve {
                host: host.to_string(),
                why: e.to_string(),
            })?;
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

    /// **The system's resolver is waited for no longer than it is given**,
    /// and answers within a wait long enough.
    #[test]
    fn the_systems_resolver_is_waited_for_no_longer_than_given() {
        let gone = System.resolve("localhost", 443, Duration::from_nanos(1));
        assert_eq!(
            gone,
            Err(Error::TimedOut {
                phase: Phase::Resolve
            })
        );
        let found = System.resolve("localhost", 443, Duration::from_secs(10));
        assert!(
            found
                .as_ref()
                .is_ok_and(|a| !a.is_empty() && a.iter().all(IpAddr::is_loopback)),
            "{found:?}"
        );
    }
}
