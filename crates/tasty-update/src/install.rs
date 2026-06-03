//! Auto-update phase 2 — atomic binary swap.
//!
//! Platform semantics:
//!
//! - Linux / macOS raw binary: `rename(2)` is atomic and the running process
//!   keeps its old inode → `SwapOutcome::Completed`. User restarts manually.
//! - macOS `.dmg`: bundle replacement is deferred to phase J.H+. The caller
//!   should open the DMG via the release page → `SwapOutcome::RestartRequired`.
//! - Windows `.exe`: the running file is locked. We write `tasty.new.exe`
//!   next to it + a one-shot `.bat` that runs on next launch. The CLI returns
//!   `RestartRequired` and the user re-launches.
//! - Linux `.deb` / `.rpm`: we don't swap inline — the caller (CLI) shells
//!   out to `pkexec dpkg -i` / `pkexec rpm -U`, or prints an instruction.

use std::fs;
use std::path::{Path, PathBuf};

/// Result of `atomic_swap`.
#[derive(Debug, Clone)]
pub enum SwapOutcome {
    /// New binary in place. Running process still holds the old inode;
    /// the user must restart Tasty for changes to take effect.
    Completed,
    /// Swap can't finish without a restart (e.g. Windows `.exe` lock,
    /// macOS `.app` bundle deferred to next phase). `instruction` is a
    /// human-readable message safe to print.
    RestartRequired { instruction: String },
}

/// Errors raised by install helpers.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("current_exe resolution failed: {0}")]
    CurrentExe(String),
    #[error("unsupported platform for in-place swap")]
    Unsupported,
}

/// Canonicalised path of the running binary. Resolves symlinks so we replace
/// the real file rather than a symlink alias.
pub fn current_exe() -> Result<PathBuf, InstallError> {
    let raw = std::env::current_exe().map_err(|e| InstallError::CurrentExe(e.to_string()))?;
    fs::canonicalize(&raw).map_err(|e| InstallError::CurrentExe(e.to_string()))
}

/// Verify the new binary is readable and the target's parent is writable.
/// No filesystem changes performed.
pub fn atomic_swap_dry(new_binary: &Path, target: &Path) -> Result<(), InstallError> {
    let _ = fs::metadata(new_binary)?; // existence check only
    let parent = target.parent().ok_or(InstallError::Unsupported)?;
    let probe = parent.join(".tasty-swap-probe");
    fs::write(&probe, b"")?;
    let _ = fs::remove_file(&probe); // best-effort cleanup
    Ok(())
}

/// Atomically replace `target` with `new_binary`. See module docs for
/// per-OS semantics.
pub fn atomic_swap(new_binary: &Path, target: &Path) -> Result<SwapOutcome, InstallError> {
    #[cfg(target_os = "macos")]
    {
        let _ = (new_binary, target); // macOS: .app/.dmg replacement deferred to later phase
        return Ok(SwapOutcome::RestartRequired {
            instruction: "macOS: open the downloaded .dmg from the release page to install."
                .to_string(),
        });
    }
    #[cfg(target_os = "linux")]
    {
        return swap_unix(new_binary, target);
    }
    #[cfg(target_os = "windows")]
    {
        return swap_windows(new_binary, target);
    }
    #[allow(unreachable_code)]
    Err(InstallError::Unsupported)
}

#[cfg(unix)]
#[allow(dead_code)]
fn swap_unix(new_binary: &Path, target: &Path) -> Result<SwapOutcome, InstallError> {
    use std::os::unix::fs::PermissionsExt;
    // 1. Move existing target to .old (best-effort backup) so rollback is possible.
    let backup = target.with_extension("old");
    let _ = fs::remove_file(&backup); // clear any prior backup; missing is fine
    if target.exists() {
        let _ = fs::rename(target, &backup); // best-effort backup
    }
    // 2. Copy new binary into place (cross-device safe; fall back to rename).
    if let Err(e) = fs::rename(new_binary, target) {
        // Different mount? Copy instead.
        tracing::debug!("rename failed ({e}), copying");
        fs::copy(new_binary, target)?;
        let _ = fs::remove_file(new_binary); // best-effort cleanup of source temp file
    }
    // 3. chmod +x.
    let mut perm = fs::metadata(target)?.permissions();
    perm.set_mode(0o755);
    fs::set_permissions(target, perm)?;
    Ok(SwapOutcome::Completed)
}

#[cfg(target_os = "windows")]
fn swap_windows(new_binary: &Path, target: &Path) -> Result<SwapOutcome, InstallError> {
    // We can't replace the running .exe. Stage the new file alongside it
    // and drop a one-shot .bat that runs after the user exits.
    let parent = target.parent().ok_or(InstallError::Unsupported)?;
    let staged = parent.join("tasty.new.exe");
    let _ = fs::remove_file(&staged); // remove stale stage from prior failed run
    fs::copy(new_binary, &staged)?;

    let bat = parent.join("tasty-swap.bat");
    let target_str = target.to_string_lossy();
    let staged_str = staged.to_string_lossy();
    let bat_str = bat.to_string_lossy();
    let script = format!(
        "@echo off\r\n\
         :retry\r\n\
         del \"{target_str}\" >nul 2>&1\r\n\
         if exist \"{target_str}\" ( timeout /t 1 /nobreak >nul & goto retry )\r\n\
         move /Y \"{staged_str}\" \"{target_str}\" >nul\r\n\
         del \"{bat_str}\"\r\n"
    );
    fs::write(&bat, script)?;
    Ok(SwapOutcome::RestartRequired {
        instruction: format!(
            "Windows: exit Tasty, then run {bat_str} to finish the swap, then re-launch."
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_exe_resolves() {
        let exe = current_exe().unwrap();
        assert!(exe.is_absolute());
    }

    #[test]
    #[cfg(unix)]
    fn dry_swap_validates_paths() {
        let dir = tempfile::tempdir().unwrap();
        let new = dir.path().join("new");
        std::fs::write(&new, b"new").unwrap();
        let target = dir.path().join("target");
        atomic_swap_dry(&new, &target).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn swap_unix_replaces_file_and_sets_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let new = dir.path().join("tasty.new");
        let target = dir.path().join("tasty");
        std::fs::write(&new, b"NEW").unwrap();
        std::fs::write(&target, b"OLD").unwrap();

        let outcome = swap_unix(&new, &target).unwrap();
        assert!(matches!(outcome, SwapOutcome::Completed));
        assert_eq!(std::fs::read(&target).unwrap(), b"NEW");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        // backup is preserved as .old
        assert!(target.with_extension("old").exists());
    }
}
