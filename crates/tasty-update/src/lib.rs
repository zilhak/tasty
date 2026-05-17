//! Auto-update phase 1: poll GitHub Releases, compare versions, surface
//! "new version available" UI. Downloading/installing is intentionally NOT
//! handled here — phase 2 work.
//!
//! Usage from the host:
//!
//! ```ignore
//! use tasty_update::{check_latest, ReleaseInfo};
//! match check_latest("zilhak", "tasty", env!("CARGO_PKG_VERSION")) {
//!     Ok(Some(info)) => /* show "new version" UI */,
//!     Ok(None) => /* up to date */,
//!     Err(e) => tracing::warn!("update check failed: {e}"),
//! }
//! ```

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
    #[error("network: {0}")]
    Network(String),
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
/// Pre-releases and drafts are skipped (caller may want stable-only).
pub fn check_latest(
    owner: &str,
    repo: &str,
    current_version: &str,
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
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    let release: GithubRelease = resp
        .into_json()
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    if release.draft || release.prerelease {
        return Ok(None);
    }

    let tag_clean = release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name);
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
}
