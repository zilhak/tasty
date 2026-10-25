//! Native TLS evidence is accepted only for the current dedicated view and exact endpoint pin.
use super::*;
use gtk::gio::prelude::*;
use sha2::{Digest, Sha256};
pub(crate) struct NativeTrustEvidence {
    pub host: String,
    pub port: u16,
    pub leaf: [u8; 32],
}
impl PreparedView {
    pub(crate) fn use_endpoint_trust(&self) -> Result<(), ContextError> {
        if !self.0.live() {
            return Err(ContextError::Retired);
        }
        self.0.endpoint_trust.set(true);
        Ok(())
    }
    pub(crate) fn trust_target(&self) -> NativeTrustTarget {
        NativeTrustTarget {
            state: Rc::downgrade(&self.0),
            epoch: self.0.epoch.get(),
        }
    }
}
#[derive(Clone)]
pub(crate) struct NativeTrustTarget {
    state: Weak<view::State>,
    epoch: u64,
}
impl NativeTrustTarget {
    pub(crate) fn current(&self) -> bool {
        self.state
            .upgrade()
            .is_some_and(|state| state.live() && state.epoch.get() == self.epoch)
    }
    pub(crate) fn accept(
        &self,
        uri: &str,
        certificate: &gio::TlsCertificate,
        flags: gio::TlsCertificateFlags,
        request_host: &str,
        request_port: u16,
        request_leaf: [u8; 32],
    ) -> Result<NativeTrustEvidence, ContextError> {
        let state = self
            .state
            .upgrade()
            .filter(|state| state.epoch.get() == self.epoch)
            .ok_or(ContextError::Retired)?;
        if !state.live() || !state.endpoint_trust.get() {
            return Err(ContextError::Retired);
        }
        if flags != gio::TlsCertificateFlags::UNKNOWN_CA {
            return Err(ContextError::Tls);
        }
        let authority = authority::https(uri).ok_or(ContextError::Policy)?;
        let expected = state.pin(&authority).ok_or(ContextError::Tls)?;
        let der = certificate.certificate().ok_or(ContextError::Tls)?;
        let leaf: [u8; 32] = Sha256::digest(der.as_ref()).into();
        if (authority.host.as_str(), authority.port, leaf)
            != (request_host, request_port, request_leaf)
            || leaf != expected
        {
            return Err(ContextError::Tls);
        }
        let trusted = state.trusted.borrow().get(&authority).copied();
        if let Some(trusted) = trusted {
            if trusted != leaf {
                return Err(ContextError::Tls);
            }
        } else {
            if state.trusted.borrow().len() >= tls::LIMIT {
                return Err(ContextError::Capacity);
            }
            let mut permit = allowance::History::reserve(&state.history, leaf)?;
            if !state.live() || state.pin(&authority) != Some(leaf) {
                return Err(ContextError::Retired);
            }
            state
                .context
                .allow_tls_certificate_for_host(certificate, &authority.host);
            permit.commit();
            state.trusted.borrow_mut().insert(authority.clone(), leaf);
        }
        Ok(NativeTrustEvidence {
            host: authority.host,
            port: authority.port,
            leaf,
        })
    }
}
