//! Prepare a ready endpoint pin before the native layer releases its held HTTPS decision.
use super::super::certificates::{AuthorityTls, CertificateError};
use super::*;
use std::sync::Weak;

pub(crate) type AuthorityPreparation =
    Pin<Box<dyn Future<Output = Result<ReadyAuthority, EndpointError>> + Send>>;

/// Retain through the native decision receipt. No trust permission is granted by this token.
pub(crate) struct ReadyAuthority {
    authority: Authority,
    certificate: Arc<AuthorityTls>,
    owner: Weak<State>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl ReadyAuthority {
    pub(crate) fn host(&self) -> &str {
        self.authority.host()
    }
    pub(crate) fn port(&self) -> u16 {
        self.authority.port()
    }
    pub(crate) fn leaf(&self) -> [u8; 32] {
        self.certificate.leaf()
    }
    pub(crate) fn is_current(&self) -> bool {
        self.owner.upgrade().is_some_and(|state| {
            (state.current)()
                && state.policy.permits("https", &self.authority)
                && state.certificates.expected_leaf(self.host(), self.port()) == Some(self.leaf())
        })
    }
}
impl Endpoint {
    /// Memory-only admission. Poll the returned work on the existing injected runtime; keep
    /// native decisions held until its current ready token matches expected_leaf. The shared
    /// budget charges unpolled work and ready receipts until Drop. No HTTP request/ID is created.
    pub(crate) fn prepare_https(
        &self,
        logical_url: &str,
    ) -> Result<AuthorityPreparation, EndpointError> {
        if !(self.state.current)() {
            return Err(EndpointError::Retired);
        }
        if logical_url.len() > tasty_ipc::html_http::URL_BYTES {
            return Err(EndpointError::Invalid);
        }
        let url = url::Url::parse(logical_url).map_err(|_| EndpointError::Invalid)?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            return Err(EndpointError::Invalid);
        }
        let authority = Authority::new(
            url.host_str().ok_or(EndpointError::Invalid)?,
            url.port_or_known_default().ok_or(EndpointError::Invalid)?,
        )
        .map_err(|_| EndpointError::Invalid)?;
        if !self.state.policy.permits("https", &authority) {
            return Err(EndpointError::Destination);
        }
        let permit = self.state.budget.preparation()?;
        let state = self.state.clone();
        Ok(Box::pin(async move {
            if !(state.current)() {
                return Err(EndpointError::Retired);
            }
            let certificate = state
                .certificates
                .certificate(authority.clone())
                .await
                .map_err(|error| match error {
                    CertificateError::Invalid => EndpointError::Invalid,
                    CertificateError::Retired => EndpointError::Retired,
                    CertificateError::Capacity => EndpointError::Capacity,
                    CertificateError::Worker => EndpointError::Worker,
                    CertificateError::Tls => EndpointError::Tls,
                })?;
            let ready = ReadyAuthority {
                authority,
                certificate,
                owner: Arc::downgrade(&state),
                _permit: permit,
            };
            if ready.is_current() {
                Ok(ready)
            } else {
                Err(EndpointError::Retired)
            }
        }))
    }
}
