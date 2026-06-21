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

use tasty_update::{NetworkErrorKind, ReleaseInfo};

/// Public-facing snapshot of the current update-check state.
#[derive(Debug, Clone, Default)]
pub struct UpdateStatus {
    /// Last successful poll result. `None` until the first poll, or after
    /// a poll that found no newer version.
    pub latest: Option<ReleaseInfo>,
    /// Concise root-cause detail from the last failed poll. `None` when the
    /// last poll succeeded.
    pub last_error: Option<String>,
    /// Network classification of the last failure, used to pick a localized
    /// category label. `None` for non-network errors (e.g. version parsing).
    pub last_error_kind: Option<NetworkErrorKind>,
    /// When the last poll completed (success or failure).
    pub last_checked: Option<Instant>,
    /// Whether a poll is currently running.
    pub in_flight: bool,
    /// Version we have already shown a notification for. Prevents repeat
    /// alerts on every hourly poll.
    pub notified_version: Option<String>,
    /// Release info awaiting an in-app notification. Main loop drains and
    /// dispatches `DomainIntent::PushNotification`, then resets to `None`.
    pub pending_notify: Option<ReleaseInfo>,
}

impl UpdateStatus {
    /// 라이브러리 표준 helper — `latest.is_some()` 의 의미 명시화 alias.
    #[allow(dead_code)]
    pub fn has_update(&self) -> bool {
        self.latest.is_some()
    }

    /// 마지막 실패를 사용자에게 보여줄 localized 메시지로 조립한다. 실패가 없으면
    /// `None`. 네트워크 분류가 있으면 번역된 카테고리 + 원본 원인을, 없으면
    /// 원본 detail 만 반환한다.
    pub fn localized_error(&self) -> Option<String> {
        let detail = self.last_error.as_ref()?;
        let Some(kind) = self.last_error_kind else {
            return Some(detail.clone());
        };
        let category = crate::i18n::t(network_kind_key(kind));
        Some(if detail.is_empty() {
            category.to_string()
        } else {
            format!("{category} — {detail}")
        })
    }
}

/// 네트워크 에러 분류 → i18n 키.
fn network_kind_key(kind: NetworkErrorKind) -> &'static str {
    match kind {
        NetworkErrorKind::Offline => "update.network.offline",
        NetworkErrorKind::Timeout => "update.network.timeout",
        NetworkErrorKind::ConnectionRefused => "update.network.connection_refused",
        NetworkErrorKind::Dns => "update.network.dns",
        NetworkErrorKind::Tls => "update.network.tls",
        NetworkErrorKind::Http => "update.network.http",
        NetworkErrorKind::BadResponse => "update.network.bad_response",
        NetworkErrorKind::Other => "update.network.other",
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
                let result = tasty_update::check_latest(owner, repo, current_version, false);
                {
                    let mut guard = shared_clone.lock().unwrap();
                    guard.in_flight = false;
                    guard.last_checked = Some(Instant::now());
                    match result {
                        Ok(Some(info)) => {
                            let is_new = guard.notified_version.as_deref() != Some(&info.version);
                            if is_new {
                                guard.pending_notify = Some(info.clone());
                            }
                            guard.latest = Some(info);
                            guard.last_error = None;
                            guard.last_error_kind = None;
                        }
                        Ok(None) => {
                            guard.latest = None;
                            guard.last_error = None;
                            guard.last_error_kind = None;
                        }
                        Err(e) => {
                            guard.last_error = Some(e.user_detail());
                            guard.last_error_kind = e.network_kind();
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
            let result = tasty_update::check_latest(owner, repo, current_version, false);
            let mut guard = shared.lock().unwrap();
            guard.in_flight = false;
            guard.last_checked = Some(Instant::now());
            match result {
                Ok(Some(info)) => {
                    let is_new = guard.notified_version.as_deref() != Some(&info.version);
                    if is_new {
                        guard.pending_notify = Some(info.clone());
                    }
                    guard.latest = Some(info);
                    guard.last_error = None;
                    guard.last_error_kind = None;
                }
                Ok(None) => {
                    guard.latest = None;
                    guard.last_error = None;
                    guard.last_error_kind = None;
                }
                Err(e) => {
                    guard.last_error = Some(e.user_detail());
                    guard.last_error_kind = e.network_kind();
                }
            }
        })
        .ok();
}
