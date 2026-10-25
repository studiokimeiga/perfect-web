//! **TLS**: rustls with the *ring* provider, its safe default protocol
//! versions (TLS 1.3 and 1.2), ALPN `http/1.1`, and the roots a client is
//! given; no client certificate, no early data, no key log.

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{CertificateError, ClientConfig, ClientConnection, RootCertStore};

use crate::origin::{Host, Origin};
use crate::{Error, Phase, Refusal, Sock, io_error};

/// **The roots a client verifies certificates against.**
#[derive(Debug, Clone)]
pub struct Roots {
    store: Arc<RootCertStore>,
}

impl Roots {
    /// **The platform's roots**: the operating system's trust store, as
    /// `rustls-native-certs` reads it (on Linux the file or directory
    /// `openssl-probe` finds, or `SSL_CERT_FILE` / `SSL_CERT_DIR`; macOS's
    /// keychains; Windows's store). The deployment's operator keeps it, and
    /// its updates reach the host at its next start. A store holding no
    /// usable certificate is refused, so a host without one fails at start,
    /// not at its first call.
    pub fn platform() -> Result<Roots, Error> {
        let found = rustls_native_certs::load_native_certs();
        let mut store = RootCertStore::empty();
        let (taken, _) = store.add_parsable_certificates(found.certs);
        if taken == 0 {
            return Err(Error::Config(format!(
                "the platform's trust store holds no certificate this client can use ({} \
                 errors reading it)",
                found.errors.len()
            )));
        }
        Ok(Roots {
            store: Arc::new(store),
        })
    }

    /// **Exactly these roots** (each a DER certificate): a deployment's
    /// private authority, or a test's.
    pub fn only<D: AsRef<[u8]>>(certificates: &[D]) -> Result<Roots, Error> {
        if certificates.is_empty() {
            return Err(Error::Config(
                "no root given: `Roots::none` is a client that verifies nothing".into(),
            ));
        }
        let mut store = RootCertStore::empty();
        for der in certificates {
            store
                .add(CertificateDer::from(der.as_ref().to_vec()))
                .map_err(|e| Error::Config(format!("a root is not one: {e}")))?;
        }
        Ok(Roots {
            store: Arc::new(store),
        })
    }

    /// **No root**: every certificate refused. A client whose grant holds
    /// plain-HTTP development origins alone needs none.
    pub fn none() -> Roots {
        Roots {
            store: Arc::new(RootCertStore::empty()),
        }
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
}

/// **The client's TLS configuration**, made once per client: its sessions
/// kept for resumption across its requests.
pub(crate) fn config(roots: &Roots) -> Result<Arc<ClientConfig>, Error> {
    let mut config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| Error::Config(format!("TLS: {e}")))?
            .with_root_certificates(roots.store.clone())
            .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// **The handshake with `origin`, over `sock`**: the server's certificate
/// verified for the origin's host (its name, sent as SNI, or its address),
/// within the socket's deadline. A refused certificate is said as the
/// [`Refusal`] it is.
pub(crate) fn handshake(
    config: &Arc<ClientConfig>,
    origin: &Origin,
    sock: &mut Sock,
) -> Result<ClientConnection, Error> {
    let name = match origin.host() {
        Host::Name(name) => ServerName::try_from(name.clone()).map_err(|e| Error::Tls {
            origin: origin.to_string(),
            why: e.to_string(),
        })?,
        Host::Ip(address) => ServerName::IpAddress((*address).into()),
    };
    let mut conn = ClientConnection::new(config.clone(), name).map_err(|e| Error::Tls {
        origin: origin.to_string(),
        why: e.to_string(),
    })?;
    while conn.is_handshaking() {
        if conn.wants_write() {
            conn.write_tls(sock)
                .map_err(|e| io_error(e, Phase::Handshake))?;
            continue;
        }
        if !conn.wants_read() {
            return Err(Error::Tls {
                origin: origin.to_string(),
                why: "the handshake stalled".into(),
            });
        }
        match conn.read_tls(sock) {
            Ok(0) => {
                return Err(Error::Tls {
                    origin: origin.to_string(),
                    why: "the server closed the connection during the handshake".into(),
                });
            }
            Ok(_) => {}
            Err(e) => return Err(io_error(e, Phase::Handshake)),
        }
        if let Err(e) = conn.process_new_packets() {
            // The alert rustls queued for the server, sent as a courtesy.
            let _ = conn.write_tls(sock);
            return Err(refused(origin, e));
        }
    }
    while conn.wants_write() {
        conn.write_tls(sock)
            .map_err(|e| io_error(e, Phase::Handshake))?;
    }
    Ok(conn)
}

/// A TLS error, as the refusal of a certificate where it is one.
fn refused(origin: &Origin, e: rustls::Error) -> Error {
    let refusal = match e {
        rustls::Error::InvalidCertificate(certificate) => match certificate {
            CertificateError::Expired | CertificateError::ExpiredContext { .. } => Refusal::Expired,
            CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => {
                Refusal::NotValidYet
            }
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
                Refusal::WrongName
            }
            CertificateError::UnknownIssuer => Refusal::UnknownIssuer,
            other => Refusal::Other(format!("{other:?}")),
        },
        other => {
            return Error::Tls {
                origin: origin.to_string(),
                why: other.to_string(),
            };
        }
    };
    Error::Certificate {
        origin: origin.to_string(),
        refusal,
    }
}
