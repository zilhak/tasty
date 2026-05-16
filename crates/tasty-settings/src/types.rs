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

/// `~/.tasty/memory.db` 의 quota / secret 정책. 모든 byte cap 은 MiB 단위 정수.
/// 0 또는 음수는 invalid 로 reject (음수는 serde 단계에서 unsigned 로 차단).
///
/// `allow_plaintext_secret` 은 keyring 부재 환경에서 secret 영역을 평문으로 폴백할지
/// 결정한다. default false — "secret 이라는 이름은 약속" 이라 사용자가 의도적으로 동의해야
/// 한다. 폴백을 허용해도 호스트는 시작 시 warning 로그 + UI 알림을 띄운다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemorySettings {
    /// 단일 entry 의 value 최대 byte (MiB).
    pub entry_max_mb: u64,
    /// 각 plugin 의 secret 영역 (`owner` 별) 최대 byte (MiB).
    pub secret_quota_mb_per_plugin: u64,
    /// Regular 영역 전체 합산 최대 byte (MiB).
    pub regular_quota_mb_total: u64,
    /// Linux 등 keyring 미가용 환경에서 secret 영역을 평문 폴백으로 운영할지.
    pub allow_plaintext_secret: bool,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            entry_max_mb: 1,
            secret_quota_mb_per_plugin: 10,
            regular_quota_mb_total: 1024,
            allow_plaintext_secret: false,
        }
    }
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
