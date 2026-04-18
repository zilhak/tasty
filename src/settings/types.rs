use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardSettings {
    pub macos_style: bool,
    pub linux_style: bool,
    pub windows_style: bool,
    /// 시스템 클립보드 변경을 감지하여 히스토리에 저장할지.
    pub history_enabled: bool,
    /// 히스토리 최대 개수.
    pub history_max: usize,
    /// 폴링 주기(ms). 재시작 필요.
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoomSettings {
    /// Ctrl+=/-/0 for zoom (Windows/Linux style)
    pub ctrl_style: bool,
    /// Alt+=/-/0 for zoom (macOS Cmd style)
    pub alt_style: bool,
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
        let (macos_style, linux_style, windows_style);
        #[cfg(target_os = "macos")]
        {
            macos_style = true;
            linux_style = false;
            windows_style = false;
        }
        #[cfg(target_os = "linux")]
        {
            macos_style = false;
            linux_style = true;
            windows_style = false;
        }
        #[cfg(any(target_os = "windows", not(any(target_os = "macos", target_os = "linux"))))]
        {
            macos_style = false;
            linux_style = false;
            windows_style = true;
        }
        Self {
            macos_style,
            linux_style,
            windows_style,
            history_enabled: true,
            history_max: 100,
            poll_interval_ms: 500,
        }
    }
}

impl Default for ZoomSettings {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                ctrl_style: false,
                alt_style: true,
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                ctrl_style: true,
                alt_style: false,
            }
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
