use serde::{Deserialize, Serialize};

/// 클립보드 히스토리 설정. 복사·붙여넣기·줌 단축키 설정은
/// `KeybindingSettings`로 옮겼고, 여기에는 히스토리 기능 관련 값만 남는다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardSettings {
    /// 시스템 클립보드 변경을 감지하여 히스토리에 저장할지.
    pub history_enabled: bool,
    /// 히스토리 최대 개수.
    pub history_max: usize,
    /// 폴링 주기(ms). 재시작 필요.
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceSettings {
    /// When enabled, only terminals with new PTY output are processed each frame
    /// instead of polling all terminals. Reduces CPU usage with many surfaces.
    /// Requires restart to apply.
    pub targeted_pty_polling: bool,
    /// When enabled, swap old scrollback lines to disk to reduce memory usage.
    /// Requires restart to apply.
    pub scrollback_disk_swap: bool,
    /// When enabled, PTY processes are only spawned when a tab is first focused,
    /// instead of at tab creation time. Reduces initial resource usage.
    /// Requires restart to apply.
    pub lazy_pty_init: bool,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            targeted_pty_polling: false,
            scrollback_disk_swap: false,
            lazy_pty_init: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub system_notification: bool,
    pub sound: bool,
    pub coalesce_ms: u64,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            history_enabled: true,
            history_max: 100,
            poll_interval_ms: 500,
        }
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            system_notification: true,
            sound: false,
            coalesce_ms: 500,
        }
    }
}
