//! Mirror-local TLS termination for a response-aware authenticated endpoint.
//! The caller pins this exact leaf and host only in its dedicated native context,
//! after SOCKS authentication. Successful TLS must be followed by HTTP parsing;
//! this acceptor never forwards encrypted streams or weakens upstream validation.
use super::{Error, Result};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{TlsAcceptor, rustls};

#[derive(Clone)]
pub struct LocalTls {
    acceptor: TlsAcceptor,
    leaf_sha256: [u8; 32],
}
impl LocalTls {
    /// Caller owns fresh per-session keys, correct host SANs and a bounded host map.
    /// Native trust must check expected leaf, host and only the unknown-issuer error.
    pub fn new(
        chain: Vec<rustls::pki_types::CertificateDer<'static>>,
        key: rustls::pki_types::PrivateKeyDer<'static>,
        certificate_bytes_limit: usize,
    ) -> Result<Self> {
        let leaf = chain
            .first()
            .filter(|c| !c.is_empty())
            .ok_or(Error::Invalid("local TLS certificate"))?;
        let bytes = chain
            .iter()
            .try_fold(key.secret_der().len(), |n, cert| n.checked_add(cert.len()))
            .ok_or(Error::Quota("local TLS certificate"))?;
        if bytes > certificate_bytes_limit {
            return Err(Error::Quota("local TLS certificate"));
        }
        let leaf_sha256 = Sha256::digest(leaf.as_ref()).into();
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| Error::Invalid("local TLS versions"))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|_| Error::Invalid("local TLS key/certificate"))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        // GIO may reuse TLS sessions across native contexts and skip their leaf
        // exception check. Require a full handshake for every local connection.
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        config.ticketer = Arc::new(NoTickets);
        config.send_tls13_tickets = 0;
        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
            leaf_sha256,
        })
    }
    pub fn leaf_sha256(&self) -> &[u8; 32] {
        &self.leaf_sha256
    }
    /// Dropping this future cancels the handshake; the owner then drops the IO.
    pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        io: S,
        deadline: Duration,
    ) -> Result<tokio_rustls::server::TlsStream<S>> {
        if deadline.is_zero() {
            return Err(Error::Invalid("local TLS deadline"));
        }
        tokio::time::timeout(deadline, self.acceptor.accept(io))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Network)
    }
}
impl std::fmt::Debug for LocalTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTls")
            .field("leaf_sha256", &self.leaf_sha256)
            .finish_non_exhaustive()
    }
}
#[derive(Debug)]
struct NoTickets;
impl rustls::server::ProducesTickets for NoTickets {
    fn enabled(&self) -> bool {
        false
    }
    fn lifetime(&self) -> u32 {
        0
    }
    fn encrypt(&self, _plain: &[u8]) -> Option<Vec<u8>> {
        None
    }
    fn decrypt(&self, _cipher: &[u8]) -> Option<Vec<u8>> {
        None
    }
}
