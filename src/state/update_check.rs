//! Host-side update-check orchestration.
//!
//! Owns a background thread that periodically polls GitHub Releases via
//! `tasty_update::check_latest`. Results are deposited into an `Arc<Mutex<_>>`
//! that `AppState` reads each frame.
//!
//! Phase-1 only — no download/install.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tasty_update::ReleaseInfo;

/// Public-facing snapshot of the current update-check state.
#[derive(Debug, Clone, Default)]
pub struct UpdateStatus {
    /// Last successful poll result. `None` until the first poll, or after
    /// a poll that found no newer version.
    pub latest: Option<ReleaseInfo>,
    /// Latest error string from a failed poll, for diagnostics.
    pub last_error: Option<String>,
    /// When the last poll completed (success or failure).
    pub last_checked: Option<Instant>,
    /// Whether a poll is currently running.
    pub in_flight: bool,
}

impl UpdateStatus {
    /// 라이브러리 표준 helper — `latest.is_some()` 의 의미 명시화 alias.
    #[allow(dead_code)]
    pub fn has_update(&self) -> bool {
        self.latest.is_some()
    }
}

/// Spawns a background polling thread. Returns a shared handle the UI can
/// read each frame. The thread runs for the lifetime of the process.
///
/// Polling cadence: one shot at startup (1s delay so the app finishes
/// initialising first), then every `interval`.
pub fn spawn_poller(
    owner: &'static str,
    repo: &'static str,
    current_version: &'static str,
    interval: Duration,
) -> Arc<Mutex<UpdateStatus>> {
    let shared = Arc::new(Mutex::new(UpdateStatus::default()));
    let shared_clone = Arc::clone(&shared);

    thread::Builder::new()
        .name("tasty-update-check".into())
        .spawn(move || {
            // Brief delay so the GUI is up before the first poll.
            thread::sleep(Duration::from_secs(1));
            loop {
                {
                    let mut guard = shared_clone.lock().unwrap();
                    guard.in_flight = true;
                }
                let result = tasty_update::check_latest(owner, repo, current_version);
                {
                    let mut guard = shared_clone.lock().unwrap();
                    guard.in_flight = false;
                    guard.last_checked = Some(Instant::now());
                    match result {
                        Ok(Some(info)) => {
                            guard.latest = Some(info);
                            guard.last_error = None;
                        }
                        Ok(None) => {
                            guard.latest = None;
                            guard.last_error = None;
                        }
                        Err(e) => {
                            guard.last_error = Some(e.to_string());
                        }
                    }
                }
                thread::sleep(interval);
            }
        })
        .expect("spawn update-check thread");

    shared
}

/// Immediately trigger a poll bypassing the cadence. Useful for a
/// "Check now" UI button. Runs on a one-shot thread; updates the same
/// shared `UpdateStatus`.
pub fn trigger_check(
    shared: Arc<Mutex<UpdateStatus>>,
    owner: &'static str,
    repo: &'static str,
    current_version: &'static str,
) {
    thread::Builder::new()
        .name("tasty-update-check-once".into())
        .spawn(move || {
            {
                let mut guard = shared.lock().unwrap();
                guard.in_flight = true;
            }
            let result = tasty_update::check_latest(owner, repo, current_version);
            let mut guard = shared.lock().unwrap();
            guard.in_flight = false;
            guard.last_checked = Some(Instant::now());
            match result {
                Ok(latest) => {
                    guard.latest = latest;
                    guard.last_error = None;
                }
                Err(e) => {
                    guard.last_error = Some(e.to_string());
                }
            }
        })
        .ok();
}
