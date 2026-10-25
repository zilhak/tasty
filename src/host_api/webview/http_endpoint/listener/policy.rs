//! Logical origin is unchanged; scheme/host/effective port are checked before route/HTTP admission.
use super::super::certificates::Authority;
use super::{EndpointConfig, EndpointError};
pub(super) struct Policy {
    entry: url::Origin,
    https: bool,
    remote: bool,
}
impl Policy {
    pub(super) fn new(config: &EndpointConfig) -> Result<Self, EndpointError> {
        if config.entry_url.len() > tasty_ipc::html_http::URL_BYTES {
            return Err(EndpointError::Invalid);
        }
        let mut entry = url::Url::parse(&config.entry_url).map_err(|_| EndpointError::Invalid)?;
        if !matches!(entry.scheme(), "http" | "https")
            || !entry.username().is_empty()
            || entry.password().is_some()
            || (entry.scheme() == "https" && !config.https)
        {
            return Err(EndpointError::Invalid);
        }
        entry.set_fragment(None);
        Ok(Self {
            entry: entry.origin(),
            https: config.https,
            remote: config.allow_remote_content,
        })
    }
    pub(super) fn permits(&self, scheme: &str, authority: &Authority) -> bool {
        if scheme == "https" && !self.https {
            return false;
        }
        if !matches!(scheme, "http" | "https") {
            return false;
        }
        let host = if authority.host().contains(':') {
            format!("[{}]", authority.host())
        } else {
            authority.host().to_owned()
        };
        let Ok(url) = url::Url::parse(&format!("{scheme}://{host}:{}/", authority.port())) else {
            return false;
        };
        self.remote || url.origin() == self.entry
    }
    pub(super) fn candidate(&self, authority: &Authority) -> bool {
        self.permits("http", authority) || self.permits("https", authority)
    }
}
