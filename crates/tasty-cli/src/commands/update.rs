//! `tasty update` — standalone CLI command (no IPC).
//!
//! Queries GitHub Releases for a newer version, downloads + SHA256-verifies
//! the right asset for the host's `(target_os, target_arch)`, and replaces
//! the running binary in place. The host doesn't need to be running.

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use clap::Args;

use tasty_update::{AssetSpec, DownloadError, InstallError, ReleaseInfo, SwapOutcome, UpdateError};

const OWNER: &str = "zilhak";
const REPO: &str = "tasty";

#[derive(Args, Debug)]
pub struct UpdateOpts {
    /// Check only, do not download / install
    #[arg(long)]
    pub check_only: bool,
    /// Skip interactive confirmation
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Allow pre-release versions
    #[arg(long)]
    pub prerelease: bool,
    /// Target a specific version tag (skip "latest" lookup) — reserved for future use
    #[arg(long)]
    pub version: Option<String>,
}

/// Entry point. Returns the process exit code.
pub fn run(opts: &UpdateOpts, current_version: &str) -> i32 {
    if opts.version.is_some() {
        eprintln!("--version pinning is not yet supported.");
        return 2;
    }

    let info = match tasty_update::check_latest(OWNER, REPO, current_version, opts.prerelease) {
        Ok(Some(info)) => info,
        Ok(None) => {
            println!("Tasty is up to date (v{current_version}).");
            return 0;
        }
        Err(e) => {
            eprintln!("update check failed: {}", format_update_error(&e));
            return 2;
        }
    };

    println!(
        "Update available: v{current} → v{new}",
        current = current_version,
        new = info.version
    );

    if opts.check_only {
        if !info.html_url.is_empty() {
            println!("Release notes: {}", info.html_url);
        }
        return 0;
    }

    if !opts.yes && !confirm(&format!("Install Tasty v{}? [y/N] ", info.version)) {
        println!("Aborted.");
        return 1;
    }

    let asset = match tasty_update::select_asset(&info) {
        Some(a) => a,
        None => {
            eprintln!(
                "No release asset matches this platform ({} / {}).",
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            return 2;
        }
    };

    match download_and_install(&info, &asset) {
        Ok(SwapOutcome::Completed) => {
            println!(
                "Updated to v{}. Please restart Tasty for the new version to take effect.",
                info.version
            );
            0
        }
        Ok(SwapOutcome::RestartRequired { instruction }) => {
            println!("Updated. Restart required:");
            println!("  {instruction}");
            0
        }
        Err(e) => {
            eprintln!("update failed: {e}");
            2
        }
    }
}

fn download_and_install(info: &ReleaseInfo, asset: &AssetSpec) -> Result<SwapOutcome, String> {
    let tmp = tempfile::NamedTempFile::new().map_err(|e| format!("tempfile: {e}"))?;
    println!("Downloading {}…", asset.name);
    tasty_update::download_to(asset, tmp.path(), progress_printer())
        .map_err(|e| format_download_error(&e))?;
    println!(); // close progress line

    println!("Fetching SHA256SUMS…");
    let sums = tasty_update::fetch_sha256_sums(info).map_err(|e| format_download_error(&e))?;
    let expected = sums
        .get(&asset.name)
        .ok_or_else(|| format!("checksum entry missing for '{}'", asset.name))?;

    println!("Verifying checksum…");
    tasty_update::verify_sha256(tmp.path(), expected).map_err(|e| format_download_error(&e))?;

    let target = tasty_update::current_exe().map_err(|e| format_install_error(&e))?;
    println!("Installing to {}…", target.display());
    install_or_print(tmp.path(), &target, &asset.name)
}

fn install_or_print(staged: &Path, target: &Path, asset_name: &str) -> Result<SwapOutcome, String> {
    if asset_name.ends_with(".deb") {
        return Ok(SwapOutcome::RestartRequired {
            instruction: format!(
                "Linux deb: run `sudo dpkg -i {}` (file kept at this temp location). Then restart Tasty.",
                staged.display()
            ),
        });
    }
    if asset_name.ends_with(".rpm") {
        return Ok(SwapOutcome::RestartRequired {
            instruction: format!(
                "Linux rpm: run `sudo rpm -U {}` (file kept at this temp location). Then restart Tasty.",
                staged.display()
            ),
        });
    }
    if asset_name.ends_with(".dmg") {
        return Ok(SwapOutcome::RestartRequired {
            instruction: format!(
                "macOS: open {} and drag Tasty.app into /Applications.",
                staged.display()
            ),
        });
    }
    if asset_name.ends_with(".msi") || asset_name.ends_with(".zip") {
        // Windows installer / archive: defer to swap_windows or user.
        tasty_update::atomic_swap(staged, target).map_err(|e| format_install_error(&e))
    } else {
        // raw binary / AppImage
        tasty_update::atomic_swap(staged, target).map_err(|e| format_install_error(&e))
    }
}

fn confirm(prompt: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        // non-interactive context: be conservative
        return false;
    }
    print!("{prompt}");
    let _ = std::io::stdout().flush(); // flush failures are not fatal for a CLI
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn progress_printer() -> impl FnMut(u64, u64) {
    let mut last_pct = u8::MAX;
    move |cur, total| {
        if total == 0 {
            // unknown size: print bytes every 256 KB
            if cur.is_multiple_of(256 * 1024) {
                print!("\r  {cur} bytes");
                let _ = std::io::stdout().flush(); // flush failures are not fatal for a CLI
            }
            return;
        }
        let pct = ((cur * 100) / total).min(100) as u8;
        if pct != last_pct {
            print!("\r  {pct:>3}%  ({cur} / {total} bytes)");
            let _ = std::io::stdout().flush(); // flush failures are not fatal for a CLI
            last_pct = pct;
        }
    }
}

fn format_update_error(e: &UpdateError) -> String {
    e.to_string()
}
fn format_download_error(e: &DownloadError) -> String {
    e.to_string()
}
fn format_install_error(e: &InstallError) -> String {
    e.to_string()
}
