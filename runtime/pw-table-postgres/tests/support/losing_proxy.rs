//! Reuses the actual PostgreSQL COMMIT-answer loss instrument from server
//! tests/tables_pg.rs, without its application fixtures. No simulated outcome.
use crate::assert_result::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
pub(crate) struct LosingProxy {
    port: u16,
    upstream: String,
    pub(crate) armed: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) lost: Arc<AtomicUsize>,
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl LosingProxy {
    /// A proxy to the database `url` names (`host:port`, after its `@`).
    pub(crate) fn start(url: &str) -> LosingProxy {
        use std::io::{Read, Write};
        use std::net::{Shutdown, TcpListener, TcpStream};
        let authority = url
            .split_once("://")
            .and_then(|(_, rest)| rest.split('/').next())
            .and_then(|a| a.rsplit('@').next())
            .map(|hp| {
                if hp.contains(':') {
                    hp.to_string()
                } else {
                    format!("{hp}:5432")
                }
            });
        let upstream = authority.assert_ok("a database URL with a host");
        let listener = TcpListener::bind("127.0.0.1:0").assert_ok("a port for the proxy");
        let port = listener.local_addr().assert_ok("its address").port();
        let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lost = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (to, arm, losses, stop) = (
            upstream.clone(),
            armed.clone(),
            lost.clone(),
            stopped.clone(),
        );
        std::thread::spawn(move || {
            for client in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let (Ok(client), Ok(server)) = (client, TcpStream::connect(&to)) else {
                    continue;
                };
                let cut = Arc::new(std::sync::atomic::AtomicBool::new(false));
                // The database's answers: passed, unless its COMMIT was cut.
                if let (Ok(mut from), Ok(mut back)) = (server.try_clone(), client.try_clone()) {
                    let (cut, losses) = (cut.clone(), losses.clone());
                    std::thread::spawn(move || {
                        let mut buf = [0u8; 16384];
                        loop {
                            let n = match from.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            if cut.load(Ordering::SeqCst) {
                                losses.fetch_add(1, Ordering::SeqCst);
                                let _ = back.shutdown(Shutdown::Both);
                                let _ = from.shutdown(Shutdown::Both);
                                break;
                            }
                            if back.write_all(&buf[..n]).is_err() {
                                break;
                            }
                        }
                    });
                }
                // The client's messages, one at a time: the first untyped.
                let (arm, cut) = (arm.clone(), cut);
                std::thread::spawn(move || {
                    let (mut from, mut onward) = (client, server);
                    let mut length = [0u8; 4];
                    if from.read_exact(&mut length).is_err() {
                        return;
                    }
                    let mut body =
                        vec![0u8; (u32::from_be_bytes(length) as usize).saturating_sub(4)];
                    if from.read_exact(&mut body).is_err()
                        || onward.write_all(&length).is_err()
                        || onward.write_all(&body).is_err()
                    {
                        return;
                    }
                    loop {
                        let mut head = [0u8; 5];
                        if from.read_exact(&mut head).is_err() {
                            break;
                        }
                        let n = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
                        let mut body = vec![0u8; n.saturating_sub(4)];
                        if from.read_exact(&mut body).is_err() {
                            break;
                        }
                        if head[0] == b'Q'
                            && body.starts_with(b"COMMIT")
                            && arm.swap(false, Ordering::SeqCst)
                        {
                            cut.store(true, Ordering::SeqCst);
                        }
                        if onward.write_all(&head).is_err() || onward.write_all(&body).is_err() {
                            break;
                        }
                    }
                    let _ = onward.shutdown(Shutdown::Write);
                });
            }
        });
        LosingProxy {
            port,
            upstream,
            armed,
            lost,
            stopped,
        }
    }

    /// `url`, its database reached through the proxy.
    pub(crate) fn url(&self, url: &str) -> String {
        url.replacen(
            &format!("@{}", self.upstream),
            &format!("@127.0.0.1:{}", self.port),
            1,
        )
        .replacen(
            &format!("@{}/", self.upstream.trim_end_matches(":5432")),
            &format!("@127.0.0.1:{}/", self.port),
            1,
        )
    }
}

impl Drop for LosingProxy {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}
