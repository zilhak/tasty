//! Auto-update phase 2 — asset selection, download, SHA256 verification.
//!
//! `select_asset` picks the right release asset for the host's `(target_os,
//! target_arch)`. `download_to` streams the asset into a file with a progress
//! callback. `fetch_sha256_sums` retrieves the matching `SHA256SUMS-*.txt` and
//! parses it; `verify_sha256` re-hashes the local file and compares.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::ReleaseInfo;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const COPY_BUF: usize = 64 * 1024;

/// Errors raised by download / verify helpers.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("no suitable asset for this platform")]
    NoAsset,
    #[error("network: {0}")]
    Network(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("checksum missing for asset '{0}' in SHA256SUMS")]
    ChecksumMissing(String),
    #[error("failed to parse SHA256SUMS file")]
    ChecksumParse,
}

/// One selected GitHub release asset (name + URL + optional pre-known size).
#[derive(Debug, Clone)]
pub struct AssetSpec {
    pub name: String,
    pub download_url: String,
}

/// Host platform key used to derive `SHA256SUMS-{key}.txt`.
///
/// Returns `"macos"`, `"windows"`, `"linux-x64"`, `"linux-arm64"` for the
/// four supported targets.
pub fn platform_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

/// Linux distro family detected from `/etc/os-release`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxFamily {
    Debian,
    Rpm,
    Other,
}

/// Read `/etc/os-release` and classify by `ID=` / `ID_LIKE=`.
pub fn detect_linux_family() -> LinuxFamily {
    let Ok(s) = std::fs::read_to_string("/etc/os-release") else {
        return LinuxFamily::Other;
    };
    classify_os_release(&s)
}

fn classify_os_release(content: &str) -> LinuxFamily {
    let mut id = String::new();
    let mut id_like = String::new();
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("ID=") {
            id = rest.trim_matches('"').to_ascii_lowercase();
        } else if let Some(rest) = line.strip_prefix("ID_LIKE=") {
            id_like = rest.trim_matches('"').to_ascii_lowercase();
        }
    }
    let any = |s: &str, needle: &str| s.split_whitespace().any(|t| t == needle);
    let is_debian =
        id == "debian" || id == "ubuntu" || any(&id_like, "debian") || any(&id_like, "ubuntu");
    let is_rpm = id == "fedora"
        || id == "rhel"
        || id == "centos"
        || id == "rocky"
        || id == "almalinux"
        || id == "opensuse"
        || id == "opensuse-leap"
        || id == "opensuse-tumbleweed"
        || any(&id_like, "fedora")
        || any(&id_like, "rhel")
        || any(&id_like, "centos")
        || any(&id_like, "suse");
    if is_debian {
        LinuxFamily::Debian
    } else if is_rpm {
        LinuxFamily::Rpm
    } else {
        LinuxFamily::Other
    }
}

/// Pick the best asset for the host's `(target_os, target_arch)`.
///
/// macOS  → `Tasty-{v}-macos.dmg`
/// windows x86_64 → `.msi` preferred, fallback `.zip`
/// linux x86_64 → deb (debian-like) / rpm (rpm-like) / AppImage / tar.gz
/// linux aarch64 → same priority, arm64 variants
pub fn select_asset(info: &ReleaseInfo) -> Option<AssetSpec> {
    select_asset_with(
        info,
        std::env::consts::OS,
        std::env::consts::ARCH,
        detect_linux_family(),
    )
}

/// Test-friendly variant: explicit OS/arch/family.
pub fn select_asset_with(
    info: &ReleaseInfo,
    target_os: &str,
    target_arch: &str,
    linux_family: LinuxFamily,
) -> Option<AssetSpec> {
    let pick = |needle: &str| info.assets.iter().find(|a| a.ends_with(needle)).cloned();
    let pick_contains = |needle: &str| info.assets.iter().find(|a| a.contains(needle)).cloned();

    let chosen: String = match (target_os, target_arch) {
        ("macos", _) => pick("-macos.dmg"),
        ("windows", _) => pick("-windows-x64.msi").or_else(|| pick("-windows-x64.zip")),
        ("linux", "x86_64") => {
            let by_family = match linux_family {
                LinuxFamily::Debian => pick_contains("_amd64.deb"),
                LinuxFamily::Rpm => pick("x86_64.rpm"),
                LinuxFamily::Other => None,
            };
            by_family
                .or_else(|| pick("-x86_64.AppImage"))
                .or_else(|| pick("-linux-x64.tar.gz"))
        }
        ("linux", "aarch64") => {
            let by_family = match linux_family {
                LinuxFamily::Debian => pick_contains("_arm64.deb"),
                LinuxFamily::Rpm => pick("aarch64.rpm"),
                LinuxFamily::Other => None,
            };
            by_family
                .or_else(|| pick("-aarch64.AppImage"))
                .or_else(|| pick("-linux-arm64.tar.gz"))
        }
        _ => None,
    }?;

    let url = derive_asset_url(&info.html_url, &info.version, &chosen)?;
    Some(AssetSpec {
        name: chosen,
        download_url: url,
    })
}

/// Build the asset download URL from the release page URL.
///
/// `html_url` is `https://github.com/{owner}/{repo}/releases/tag/v{version}`.
/// The asset URL is `https://github.com/{owner}/{repo}/releases/download/v{version}/{asset}`.
fn derive_asset_url(html_url: &str, version: &str, asset: &str) -> Option<String> {
    let base = html_url.rsplit_once("/releases/")?.0;
    let tag = if html_url.contains(&format!("/v{version}")) {
        format!("v{version}")
    } else {
        version.to_string()
    };
    Some(format!("{base}/releases/download/{tag}/{asset}"))
}

/// Download `asset.download_url` into `dest`, reporting progress as
/// `(bytes_so_far, total_bytes)`. `total_bytes` is 0 when Content-Length
/// is missing.
pub fn download_to(
    asset: &AssetSpec,
    dest: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), DownloadError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(&format!("tasty-update/{}", env!("CARGO_PKG_VERSION")))
        .build();
    let resp = agent
        .get(&asset.download_url)
        .call()
        .map_err(|e| DownloadError::Network(e.to_string()))?;

    let total = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut reader = resp.into_reader();
    let mut file = File::create(dest)?;
    let mut buf = vec![0u8; COPY_BUF];
    let mut done: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| DownloadError::Network(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    file.flush()?;
    Ok(())
}

/// Hash `file` with SHA-256 and compare against `expected_hex` (case-insensitive).
pub fn verify_sha256(file: &Path, expected_hex: &str) -> Result<(), DownloadError> {
    let mut f = File::open(file)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex_encode(&hasher.finalize());
    if actual.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(DownloadError::ChecksumMismatch {
            expected: expected_hex.to_string(),
            actual,
        })
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut s, "{b:02x}"); // writing to String is infallible
    }
    s
}

/// Download `SHA256SUMS-{platform_key}.txt` from the same release and parse
/// it into a `{asset_name -> hex_hash}` map.
pub fn fetch_sha256_sums(info: &ReleaseInfo) -> Result<HashMap<String, String>, DownloadError> {
    let sums_name = format!("SHA256SUMS-{}.txt", platform_key());
    let url = derive_asset_url(&info.html_url, &info.version, &sums_name)
        .ok_or(DownloadError::NoAsset)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(&format!("tasty-update/{}", env!("CARGO_PKG_VERSION")))
        .build();
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| DownloadError::Network(e.to_string()))?
        .into_string()
        .map_err(|e| DownloadError::Network(e.to_string()))?;
    parse_sha256_sums(&body)
}

/// Parse a SHA256SUMS-style file: `{hex}  {filename}` per line.
/// Leading `*` (binary marker) on filename is stripped.
pub fn parse_sha256_sums(body: &str) -> Result<HashMap<String, String>, DownloadError> {
    let mut out = HashMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let hash = parts.next().ok_or(DownloadError::ChecksumParse)?;
        let rest = parts.next().ok_or(DownloadError::ChecksumParse)?.trim();
        let name = rest.trim_start_matches('*').to_string();
        if hash.len() != 64 {
            return Err(DownloadError::ChecksumParse);
        }
        out.insert(name, hash.to_string());
    }
    if out.is_empty() {
        return Err(DownloadError::ChecksumParse);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    fn mk_info(version: &str, assets: &[&str]) -> ReleaseInfo {
        ReleaseInfo {
            version: version.to_string(),
            parsed: Version::parse(version).unwrap(),
            html_url: format!("https://github.com/zilhak/tasty/releases/tag/v{version}"),
            body: String::new(),
            assets: assets.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn full_asset_list(v: &str) -> Vec<String> {
        vec![
            format!("Tasty-{v}-macos.dmg"),
            format!("tasty-{v}-windows-x64.zip"),
            format!("tasty-{v}-windows-x64.msi"),
            format!("tasty-{v}-linux-x64.tar.gz"),
            format!("tasty_{v}-1_amd64.deb"),
            format!("tasty-{v}-1.x86_64.rpm"),
            format!("Tasty-{v}-x86_64.AppImage"),
            format!("tasty-{v}-linux-arm64.tar.gz"),
            format!("tasty_{v}-1_arm64.deb"),
            format!("tasty-{v}-1.aarch64.rpm"),
            format!("Tasty-{v}-aarch64.AppImage"),
        ]
    }

    #[test]
    fn select_asset_macos_picks_dmg() {
        let v = "0.7.0";
        let assets = full_asset_list(v);
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "macos", "aarch64", LinuxFamily::Other).unwrap();
        assert_eq!(spec.name, format!("Tasty-{v}-macos.dmg"));
        assert!(spec.download_url.ends_with(&format!("/v{v}/{}", spec.name)));
    }

    #[test]
    fn select_asset_windows_prefers_msi_over_zip() {
        let v = "0.7.0";
        let assets = full_asset_list(v);
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "windows", "x86_64", LinuxFamily::Other).unwrap();
        assert_eq!(spec.name, format!("tasty-{v}-windows-x64.msi"));
    }

    #[test]
    fn select_asset_windows_zip_fallback_when_no_msi() {
        let v = "0.7.0";
        let assets = [format!("tasty-{v}-windows-x64.zip")];
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "windows", "x86_64", LinuxFamily::Other).unwrap();
        assert_eq!(spec.name, format!("tasty-{v}-windows-x64.zip"));
    }

    #[test]
    fn select_asset_linux_x86_64_debian() {
        let v = "0.7.0";
        let assets = full_asset_list(v);
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "linux", "x86_64", LinuxFamily::Debian).unwrap();
        assert_eq!(spec.name, format!("tasty_{v}-1_amd64.deb"));
    }

    #[test]
    fn select_asset_linux_x86_64_rpm() {
        let v = "0.7.0";
        let assets = full_asset_list(v);
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "linux", "x86_64", LinuxFamily::Rpm).unwrap();
        assert_eq!(spec.name, format!("tasty-{v}-1.x86_64.rpm"));
    }

    #[test]
    fn select_asset_linux_x86_64_appimage_fallback() {
        let v = "0.7.0";
        let assets = full_asset_list(v);
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "linux", "x86_64", LinuxFamily::Other).unwrap();
        assert_eq!(spec.name, format!("Tasty-{v}-x86_64.AppImage"));
    }

    #[test]
    fn select_asset_linux_aarch64_debian() {
        let v = "0.7.0";
        let assets = full_asset_list(v);
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "linux", "aarch64", LinuxFamily::Debian).unwrap();
        assert_eq!(spec.name, format!("tasty_{v}-1_arm64.deb"));
    }

    #[test]
    fn select_asset_linux_aarch64_rpm() {
        let v = "0.7.0";
        let assets = full_asset_list(v);
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "linux", "aarch64", LinuxFamily::Rpm).unwrap();
        assert_eq!(spec.name, format!("tasty-{v}-1.aarch64.rpm"));
    }

    #[test]
    fn select_asset_linux_aarch64_appimage_fallback() {
        let v = "0.7.0";
        let assets = full_asset_list(v);
        let info = mk_info(v, &assets.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let spec = select_asset_with(&info, "linux", "aarch64", LinuxFamily::Other).unwrap();
        assert_eq!(spec.name, format!("Tasty-{v}-aarch64.AppImage"));
    }

    #[test]
    fn parse_sums_basic() {
        let body = "\
abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789  Tasty-0.7.0-macos.dmg
deadbeef11111111deadbeef22222222deadbeef33333333deadbeef44444444 *Tasty-0.7.0-x86_64.AppImage
";
        let map = parse_sha256_sums(body).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("Tasty-0.7.0-macos.dmg").unwrap(),
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        );
        assert!(map.contains_key("Tasty-0.7.0-x86_64.AppImage"));
    }

    #[test]
    fn parse_sums_rejects_short_hash() {
        let body = "deadbeef  Tasty-0.7.0-macos.dmg\n";
        assert!(parse_sha256_sums(body).is_err());
    }

    #[test]
    fn verify_sha256_match_and_mismatch() {
        use std::io::Write;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(tmp.path())
                .unwrap();
            f.write_all(b"hello world").unwrap();
        }
        // SHA-256("hello world")
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        verify_sha256(tmp.path(), expected).unwrap();
        let bad = verify_sha256(tmp.path(), &"f".repeat(64));
        assert!(matches!(bad, Err(DownloadError::ChecksumMismatch { .. })));
    }

    #[test]
    fn classify_os_release_debian() {
        let s = "ID=ubuntu\nID_LIKE=debian\n";
        assert_eq!(classify_os_release(s), LinuxFamily::Debian);
    }

    #[test]
    fn classify_os_release_fedora() {
        let s = "ID=fedora\n";
        assert_eq!(classify_os_release(s), LinuxFamily::Rpm);
    }

    #[test]
    fn classify_os_release_unknown_falls_back() {
        let s = "ID=arch\n";
        assert_eq!(classify_os_release(s), LinuxFamily::Other);
    }

    #[test]
    fn platform_key_matches_target() {
        let k = platform_key();
        if cfg!(target_os = "macos") {
            assert_eq!(k, "macos");
        } else if cfg!(target_os = "windows") {
            assert_eq!(k, "windows");
        } else if cfg!(target_arch = "aarch64") {
            assert_eq!(k, "linux-arm64");
        } else {
            assert_eq!(k, "linux-x64");
        }
    }

    #[test]
    fn derive_asset_url_basic() {
        let url = derive_asset_url(
            "https://github.com/zilhak/tasty/releases/tag/v0.7.0",
            "0.7.0",
            "Tasty-0.7.0-macos.dmg",
        )
        .unwrap();
        assert_eq!(
            url,
            "https://github.com/zilhak/tasty/releases/download/v0.7.0/Tasty-0.7.0-macos.dmg"
        );
    }
}
