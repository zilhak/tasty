//! Cross-platform TCP listening port enumeration for tasty.
//!
//! Given a set of process IDs (typically a PTY shell and its descendants),
//! returns the TCP ports those processes are listening on.
//!
//! Implementation per OS:
//! - Linux: parse `/proc/net/tcp` + `/proc/net/tcp6`, match inodes against `/proc/{pid}/fd/*`
//! - macOS: `lsof -iTCP -sTCP:LISTEN -nP -p <pids>` subprocess (no good API in stable Rust)
//! - Windows: `GetExtendedTcpTable` Win32 API with `TCP_TABLE_OWNER_PID_LISTENER`

use std::collections::HashSet;
use std::net::IpAddr;
use std::time::{Duration, Instant};

mod cache;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod tree;
#[cfg(windows)]
mod windows;

pub use cache::PortScanCache;
pub use tree::collect_descendant_pids;

/// A TCP listening port observed for one process.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListeningPort {
    /// PID of the process holding the listening socket.
    pub pid: u32,
    /// Local listening port.
    pub port: u16,
    /// Local bind address (e.g. `0.0.0.0`, `127.0.0.1`, `::1`).
    pub addr: IpAddr,
}

/// Scan TCP listening ports owned by any of the given PIDs.
///
/// Returns a sorted, deduplicated list (by port + pid + addr). Empty `pids` yields empty.
/// Errors during enumeration are logged at warn level and result in an empty vec.
pub fn scan_for_pids(pids: &HashSet<u32>) -> Vec<ListeningPort> {
    if pids.is_empty() {
        return Vec::new();
    }

    let mut found = scan_impl(pids);
    // Dedup + sort for stable UI ordering.
    found.sort_by_key(|p| (p.port, p.pid, p.addr.to_string()));
    found.dedup();
    found
}

#[cfg(target_os = "linux")]
fn scan_impl(pids: &HashSet<u32>) -> Vec<ListeningPort> {
    linux::scan(pids).unwrap_or_else(|e| {
        tracing::warn!("portscan(linux) failed: {e}");
        Vec::new()
    })
}

#[cfg(target_os = "macos")]
fn scan_impl(pids: &HashSet<u32>) -> Vec<ListeningPort> {
    macos::scan(pids).unwrap_or_else(|e| {
        tracing::warn!("portscan(macos) failed: {e}");
        Vec::new()
    })
}

#[cfg(windows)]
fn scan_impl(pids: &HashSet<u32>) -> Vec<ListeningPort> {
    windows::scan(pids).unwrap_or_else(|e| {
        tracing::warn!("portscan(windows) failed: {e}");
        Vec::new()
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn scan_impl(_pids: &HashSet<u32>) -> Vec<ListeningPort> {
    Vec::new()
}

/// Default TTL for the per-surface cache. Repeated scans within this window
/// reuse the previous result.
pub const DEFAULT_TTL: Duration = Duration::from_secs(5);

/// One entry in the scan-result cache. Public so callers (UI) can read the
/// last-good result while a refresh is pending.
#[derive(Debug, Clone)]
pub struct CachedScan {
    pub ports: Vec<ListeningPort>,
    pub at: Instant,
}
