//! Reclaims swap files left by processes that ended without running `Drop` (SIGKILL, crash,
//! power loss). Each file name carries its owner's PID, so a later run removes only the files
//! whose owner has certainly exited. Files of live owners, owners whose state cannot be
//! determined, and names in any other format are kept.

use std::path::Path;

/// Whether the process that owns a swap file is still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Exited,
    /// The OS did not say (for example access denied on Windows). The file is kept.
    Unknown,
}

/// What one reclaim pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReclaimReport {
    pub removed: usize,
    pub kept_alive: usize,
    pub kept_unknown: usize,
    pub failed: usize,
}

impl ReclaimReport {
    fn add(&mut self, other: Self) {
        self.removed += other.removed;
        self.kept_alive += other.kept_alive;
        self.kept_unknown += other.kept_unknown;
        self.failed += other.failed;
    }
}

/// Both build profiles share the PID space, so a pass covers the debug and the release folder.
pub fn reclaim_orphans() -> ReclaimReport {
    let mut report = ReclaimReport::default();
    for subdir in super::SUBDIRS {
        // reason: shared swap folders; only names with an exited owner PID are removed.
        let dir = std::env::temp_dir().join(subdir);
        report.add(reclaim_in(&dir, process_liveness));
    }
    report
}

/// Runs [`reclaim_orphans`] on its own thread so a large temp folder never delays startup.
pub fn spawn_orphan_reclaim() {
    let spawned = std::thread::Builder::new()
        .name("scrollback-reclaim".into())
        .spawn(|| {
            let report = reclaim_orphans();
            if report != ReclaimReport::default() {
                tracing::info!(?report, "disk scrollback orphan reclaim");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("disk scrollback orphan reclaim not started: {e}");
    }
}

/// One folder. `liveness` decides per owner PID; this process always counts as alive.
pub fn reclaim_in(dir: &Path, liveness: impl Fn(u32) -> Liveness) -> ReclaimReport {
    let mut report = ReclaimReport::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return report,
        Err(e) => {
            tracing::warn!(
                "disk scrollback reclaim: cannot list {}: {e}",
                dir.display()
            );
            return report;
        }
    };
    let own = std::process::id();
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(owner_pid) else {
            continue;
        };
        // Symlinks and folders with a matching name are not swap files.
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let state = if pid == own {
            Liveness::Alive
        } else {
            liveness(pid)
        };
        report.add(settle(&entry.path(), state));
    }
    report
}

/// Removes one swap file only when its owner certainly exited.
fn settle(path: &Path, state: Liveness) -> ReclaimReport {
    let mut report = ReclaimReport::default();
    match state {
        Liveness::Alive => report.kept_alive = 1,
        Liveness::Unknown => report.kept_unknown = 1,
        Liveness::Exited => match std::fs::remove_file(path) {
            Ok(()) => report.removed = 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                report.failed = 1;
                tracing::warn!(
                    "disk scrollback reclaim: cannot remove {}: {e}",
                    path.display()
                );
            }
        },
    }
    report
}

/// The owner PID of `surface-<pid>-<sid>-<nonce>.scrollback`, or None for any other name.
pub(crate) fn owner_pid(name: &str) -> Option<u32> {
    let fields = name.strip_prefix("surface-")?.strip_suffix(".scrollback")?;
    let mut parts = fields.split('-');
    let pid = parts.next()?;
    let sid = parts.next()?;
    let nonce = parts.next()?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if parts.next().is_some() || !digits(pid) || !digits(sid) || !digits(nonce) {
        return None;
    }
    pid.parse().ok()
}

/// `kill(pid, 0)`: success or EPERM means the process exists, ESRCH means it does not.
#[cfg(unix)]
pub fn process_liveness(pid: u32) -> Liveness {
    let Ok(raw) = libc::pid_t::try_from(pid) else {
        return Liveness::Unknown;
    };
    if raw <= 0 {
        return Liveness::Unknown;
    }
    // SAFETY: signal 0 only checks existence and permission; nothing is delivered.
    if unsafe { libc::kill(raw, 0) } == 0 {
        return Liveness::Alive;
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Liveness::Exited,
        Some(libc::EPERM) => Liveness::Alive,
        _ => Liveness::Unknown,
    }
}

/// `OpenProcess` fails with ERROR_INVALID_PARAMETER only when no such process exists. Any other
/// failure, such as access denied, leaves the state unknown instead of treating it as exited.
#[cfg(windows)]
pub fn process_liveness(pid: u32) -> Liveness {
    use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain query; the handle is closed below on every path that opened it.
    let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(handle) => handle,
        Err(e) if e.code() == ERROR_INVALID_PARAMETER.to_hresult() => return Liveness::Exited,
        Err(_) => return Liveness::Unknown,
    };
    let mut code = 0u32;
    // SAFETY: `handle` is the process handle opened above and `code` is a valid out pointer.
    let queried = unsafe { GetExitCodeProcess(handle, &mut code) };
    // SAFETY: closes the handle opened above exactly once.
    if let Err(e) = unsafe { CloseHandle(handle) } {
        tracing::trace!("disk scrollback reclaim: CloseHandle failed: {e}");
    }
    match queried {
        Ok(()) if code == STILL_ACTIVE.0 as u32 => Liveness::Alive,
        Ok(()) => Liveness::Exited,
        Err(_) => Liveness::Unknown,
    }
}

#[cfg(not(any(unix, windows)))]
pub fn process_liveness(_pid: u32) -> Liveness {
    Liveness::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private folder removed at the end of the test (the crate has no tempfile dependency).
    struct TestDir(std::path::PathBuf);
    impl TestDir {
        fn new(tag: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "tasty-reclaim-test-{}-{tag}-{n}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            if let Err(e) = std::fs::remove_dir_all(&self.0) {
                tracing::warn!("test folder cleanup failed: {e}");
            }
        }
    }

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    fn present(dir: &Path, name: &str) -> bool {
        dir.join(name).exists()
    }

    #[test]
    fn only_strict_swap_names_carry_an_owner() {
        assert_eq!(owner_pid("surface-42-7-1.scrollback"), Some(42));
        for name in [
            "unrelated.txt",
            "surface-42-7.scrollback",
            "surface-42-7-1-9.scrollback",
            "surface-x-7-1.scrollback",
            "surface--7-1.scrollback",
            "surface-42-7-1.scrollback.bak",
            "prefix-surface-42-7-1.scrollback",
        ] {
            assert_eq!(owner_pid(name), None, "{name}");
        }
    }

    /// Exited owners lose their files; live, unknown and own files and other names stay.
    #[test]
    fn reclaim_removes_only_files_whose_owner_certainly_exited() {
        let dir = TestDir::new("mixed");
        let own = std::process::id();
        let names = [
            "surface-101-1-1.scrollback".to_string(),
            "surface-202-1-1.scrollback".to_string(),
            "surface-303-1-1.scrollback".to_string(),
            format!("surface-{own}-1-1.scrollback"),
            "unrelated.txt".to_string(),
        ];
        for name in &names {
            touch(dir.path(), name);
        }
        std::fs::create_dir(dir.path().join("surface-101-9-9.scrollback")).unwrap();
        let report = reclaim_in(dir.path(), |pid| match pid {
            101 => Liveness::Exited,
            202 => Liveness::Alive,
            303 => Liveness::Unknown,
            other => panic!("own pid {other} must not be probed"),
        });
        assert_eq!(
            report,
            ReclaimReport {
                removed: 1,
                kept_alive: 2,
                kept_unknown: 1,
                failed: 0
            }
        );
        assert!(!present(dir.path(), &names[0]));
        for name in &names[1..] {
            assert!(present(dir.path(), name), "{name}");
        }
        assert!(dir.path().join("surface-101-9-9.scrollback").is_dir());
    }

    #[test]
    fn a_missing_folder_is_an_empty_pass() {
        let dir = TestDir::new("absent");
        let report = reclaim_in(&dir.path().join("absent"), |_| Liveness::Exited);
        assert_eq!(report, ReclaimReport::default());
    }

    /// A child that was spawned and reaped has certainly exited; this process is alive.
    #[cfg(any(unix, windows))]
    #[test]
    fn the_real_probe_tells_a_reaped_child_from_this_process() {
        assert_eq!(process_liveness(std::process::id()), Liveness::Alive);
        #[cfg(unix)]
        let mut child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert_eq!(process_liveness(pid), Liveness::Exited);
    }
}
