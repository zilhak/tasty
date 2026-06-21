//! Auto-update: poll GitHub Releases, compare versions, download + verify
//! + atomically swap the binary.
//!
//! Usage from the host:
//!
//! ```ignore
//! use tasty_update::{check_latest, ReleaseInfo};
//! match check_latest("zilhak", "tasty", env!("CARGO_PKG_VERSION"), false) {
//!     Ok(Some(info)) => /* show "new version" UI */,
//!     Ok(None) => /* up to date */,
//!     Err(e) => tracing::warn!("update check failed: {e}"),
//! }
//! ```

pub mod download;
pub mod install;

pub use download::{
    AssetSpec, DownloadError, LinuxFamily, detect_linux_family, download_to, fetch_sha256_sums,
    parse_sha256_sums, platform_key, select_asset, select_asset_with, verify_sha256,
};
pub use install::{InstallError, SwapOutcome, atomic_swap, atomic_swap_dry, current_exe};

use std::time::Duration;

use semver::Version;
use serde::Deserialize;

/// Latest release info, returned when a newer version than `current` exists.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    /// Tag without leading 'v' (e.g. "0.6.0").
    pub version: String,
    /// Parsed `Version`, guaranteed to be > current.
    pub parsed: Version,
    /// Release page URL (browser target).
    pub html_url: String,
    /// Body of the release (markdown notes), as posted on GitHub.
    pub body: String,
    /// Asset filenames, useful later for phase-2 picker.
    pub assets: Vec<String>,
}

/// Coarse classification of a failed network poll, suitable for picking a
/// localized message on the host side. The host maps each variant to a
/// translation key; CLI shows the raw [`UpdateError`] Display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkErrorKind {
    /// Host unreachable / no route — typically offline.
    Offline,
    /// Connection or read timed out.
    Timeout,
    /// Connection actively refused or reset by the peer.
    ConnectionRefused,
    /// DNS resolution failed.
    Dns,
    /// TLS or proxy negotiation problem.
    Tls,
    /// Server responded with an HTTP error status.
    Http,
    /// Server replied but the response was malformed / unparseable.
    BadResponse,
    /// Anything not covered above.
    Other,
}

/// Errors raised by `check_latest`.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("invalid current version: {0}")]
    InvalidCurrent(semver::Error),
    #[error("invalid remote tag '{tag}': {source}")]
    InvalidRemote {
        tag: String,
        #[source]
        source: semver::Error,
    },
    /// A network/transport failure. `detail` is the concise root cause (the
    /// deepest source error), already stripped of ureq's redundant wrapper
    /// chain. `kind` drives localized messaging on the host.
    #[error("network: {detail}")]
    Network {
        kind: NetworkErrorKind,
        detail: String,
    },
}

impl UpdateError {
    /// The network classification, if this is a [`UpdateError::Network`].
    pub fn network_kind(&self) -> Option<NetworkErrorKind> {
        match self {
            UpdateError::Network { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// A concise, human-readable detail for the UI: the root cause for network
    /// errors (no redundant ureq chrome), or the full Display otherwise.
    pub fn user_detail(&self) -> String {
        match self {
            UpdateError::Network { detail, .. } => detail.clone(),
            other => other.to_string(),
        }
    }
}

/// Walk the `source()` chain to the deepest error and return its message.
///
/// ureq 2.x wraps a socket `io::Error` in *two* nested `Transport`s, so the
/// naive `e.to_string()` repeats the kind ("Network Error: Network Error: …")
/// and prepends the URL. The deepest source is the original OS error — the only
/// part a user can act on.
fn root_cause_message(e: &(dyn std::error::Error + 'static)) -> String {
    let mut deepest = e;
    while let Some(src) = deepest.source() {
        deepest = src;
    }
    deepest.to_string()
}

/// Find the deepest `io::Error` in the source chain and map its kind to a
/// `NetworkErrorKind`. Returns `None` if no `io::Error` is present.
fn classify_io(e: &(dyn std::error::Error + 'static)) -> Option<NetworkErrorKind> {
    use std::io::ErrorKind as Io;

    let mut io_kind = None;
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(c) = cur {
        if let Some(io) = c.downcast_ref::<std::io::Error>() {
            io_kind = Some(io.kind());
        }
        cur = c.source();
    }

    io_kind.map(|k| match k {
        Io::TimedOut => NetworkErrorKind::Timeout,
        Io::ConnectionRefused | Io::ConnectionReset | Io::ConnectionAborted => {
            NetworkErrorKind::ConnectionRefused
        }
        Io::HostUnreachable
        | Io::NetworkUnreachable
        | Io::NetworkDown
        | Io::NotConnected
        | Io::AddrNotAvailable => NetworkErrorKind::Offline,
        _ => NetworkErrorKind::Other,
    })
}

/// Classify a ureq error into `(kind, concise detail)`.
fn classify_ureq(e: &ureq::Error) -> (NetworkErrorKind, String) {
    use ureq::ErrorKind as K;

    let detail = root_cause_message(e);
    let kind = match e {
        ureq::Error::Status(code, _) => {
            return (NetworkErrorKind::Http, format!("HTTP {code}"));
        }
        ureq::Error::Transport(t) => match t.kind() {
            K::Dns => NetworkErrorKind::Dns,
            K::ConnectionFailed | K::ProxyConnect => {
                classify_io(e).unwrap_or(NetworkErrorKind::Offline)
            }
            K::Io => classify_io(e).unwrap_or(NetworkErrorKind::Other),
            K::InsecureRequestHttpsOnly | K::InvalidProxyUrl | K::ProxyUnauthorized => {
                NetworkErrorKind::Tls
            }
            K::BadStatus | K::BadHeader | K::TooManyRedirects => NetworkErrorKind::BadResponse,
            K::HTTP => NetworkErrorKind::Http,
            K::InvalidUrl | K::UnknownScheme => NetworkErrorKind::Other,
        },
    };
    (kind, detail)
}

/// Build a [`UpdateError::Network`] from a ureq error.
fn network_err(e: &ureq::Error) -> UpdateError {
    let (kind, detail) = classify_ureq(e);
    UpdateError::Network { kind, detail }
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
}

const API_TIMEOUT: Duration = Duration::from_secs(10);

/// Query `https://api.github.com/repos/{owner}/{repo}/releases/latest` and
/// return `Some(ReleaseInfo)` iff the parsed remote tag is strictly greater
/// than `current_version`.
///
/// Drafts are always skipped. Pre-releases are skipped unless
/// `allow_prerelease` is `true` (CLI `--prerelease` flag).
pub fn check_latest(
    owner: &str,
    repo: &str,
    current_version: &str,
    allow_prerelease: bool,
) -> Result<Option<ReleaseInfo>, UpdateError> {
    let current = Version::parse(current_version).map_err(UpdateError::InvalidCurrent)?;
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");

    let agent = ureq::AgentBuilder::new()
        .timeout(API_TIMEOUT)
        .user_agent(&format!("tasty-update/{}", env!("CARGO_PKG_VERSION")))
        .build();
    let resp = agent
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| network_err(&e))?;

    let release: GithubRelease = resp.into_json().map_err(|e| UpdateError::Network {
        kind: NetworkErrorKind::BadResponse,
        detail: e.to_string(),
    })?;

    if release.draft {
        return Ok(None);
    }
    if release.prerelease && !allow_prerelease {
        return Ok(None);
    }

    let tag_clean = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let parsed = Version::parse(tag_clean).map_err(|e| UpdateError::InvalidRemote {
        tag: release.tag_name.clone(),
        source: e,
    })?;

    if parsed <= current {
        return Ok(None);
    }

    Ok(Some(ReleaseInfo {
        version: tag_clean.to_string(),
        parsed,
        html_url: release.html_url,
        body: release.body,
        assets: release.assets.into_iter().map(|a| a.name).collect(),
    }))
}

/// Pure helper for tests / offline use: compare two version strings and
/// return Ok(true) iff `remote > current`. Leading 'v' is stripped from remote.
pub fn is_newer(current: &str, remote_tag: &str) -> Result<bool, UpdateError> {
    let current = Version::parse(current).map_err(UpdateError::InvalidCurrent)?;
    let remote_clean = remote_tag.strip_prefix('v').unwrap_or(remote_tag);
    let remote = Version::parse(remote_clean).map_err(|e| UpdateError::InvalidRemote {
        tag: remote_tag.to_string(),
        source: e,
    })?;
    Ok(remote > current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_strips_v_prefix() {
        assert!(is_newer("0.5.0", "v0.6.0").unwrap());
        assert!(is_newer("0.5.0", "0.6.0").unwrap());
    }

    #[test]
    fn same_or_older_returns_false() {
        assert!(!is_newer("0.6.0", "v0.6.0").unwrap());
        assert!(!is_newer("0.6.0", "v0.5.9").unwrap());
    }

    #[test]
    fn prerelease_comparison() {
        // 0.6.0-rc1 < 0.6.0 (semver rule)
        assert!(is_newer("0.6.0-rc1", "0.6.0").unwrap());
        assert!(!is_newer("0.6.0", "0.6.0-rc1").unwrap());
    }

    #[test]
    fn invalid_remote_propagates() {
        let err = is_newer("0.5.0", "not-a-version").unwrap_err();
        assert!(matches!(err, UpdateError::InvalidRemote { .. }));
    }

    fn ureq_io(kind: std::io::ErrorKind, msg: &str) -> ureq::Error {
        ureq::Error::from(std::io::Error::new(kind, msg.to_string()))
    }

    #[test]
    fn classify_maps_timeout_and_keeps_root_detail() {
        let (kind, detail) = classify_ureq(&ureq_io(std::io::ErrorKind::TimedOut, "timed out"));
        assert_eq!(kind, NetworkErrorKind::Timeout);
        // Concise root cause only — no doubled "Network Error" chrome.
        assert_eq!(detail, "timed out");
    }

    #[test]
    fn classify_maps_connection_refused() {
        let (kind, _) =
            classify_ureq(&ureq_io(std::io::ErrorKind::ConnectionRefused, "refused"));
        assert_eq!(kind, NetworkErrorKind::ConnectionRefused);
    }

    #[test]
    fn classify_maps_unreachable_to_offline() {
        let (kind, _) =
            classify_ureq(&ureq_io(std::io::ErrorKind::HostUnreachable, "no route"));
        assert_eq!(kind, NetworkErrorKind::Offline);
    }

    #[test]
    fn network_err_exposes_kind_and_detail() {
        let e = network_err(&ureq_io(std::io::ErrorKind::TimedOut, "timed out"));
        assert_eq!(e.network_kind(), Some(NetworkErrorKind::Timeout));
        assert_eq!(e.user_detail(), "timed out");
        // CLI Display stays concise.
        assert_eq!(e.to_string(), "network: timed out");
    }

    #[test]
    fn non_network_error_has_no_kind() {
        let err = is_newer("0.5.0", "not-a-version").unwrap_err();
        assert_eq!(err.network_kind(), None);
    }
}
