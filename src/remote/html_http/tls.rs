//! Response-aware HTTP/TLS transport; certificate and hostname verification stay enabled.
use super::{Error, Limits, Result};

pub(super) fn client(limits: &Limits) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .http1_only()
        .pool_max_idle_per_host(0)
        .connect_timeout(limits.connect_timeout)
        .read_timeout(limits.read_timeout)
        .tls_backend_rustls()
        .tls_version_min(reqwest::tls::Version::TLS_1_2)
        .build()
        .map_err(|_| Error::Network)
}
