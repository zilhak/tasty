use serde::{Deserialize, Serialize};
use tasty_type_geometry::length::LogicalPx;

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
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            targeted_pty_polling: true,
            scrollback_disk_swap: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub sound: bool,
    pub coalesce_ms: u64,
}

/// Accessibility 관련 토글. OS 자동 감지(Phase 2)는 미구현 — 현재는 모두 수동 설정.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct AccessibilitySettings {
    /// 활성 시 모든 UI 페이드/슬라이드 애니메이션을 즉시 끝낸다. 토스트 페이드인/페이드아웃은 0ms로 적용.
    pub reduced_motion: bool,
}

/// Modifier 키 홀드 시 표시되는 단축키 안내 오버레이의 설정 슬롯.
/// on/off 토글과 사용자가 이동/리사이즈한 위치·크기를 영속한다.
///
/// 지오메트리(`pos`/`size`)는 접근성 의미가 아닌 오버레이 UI 상태이므로
/// `AccessibilitySettings` 와 분리한 전용 루트 섹션으로 둔다.
/// 저장값은 윈도우 축소로 화면 밖이 되어도 불변이며, 클램프는 렌더 단계 책임(이 계층은 저장까지만).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModifierHintSettings {
    /// 오버레이 표시 on/off. 기본 true (거치적거리면 사용자가 끈다).
    pub enabled: bool,
    /// 사용자가 이동한 위치. `None` = 기본 위치(렌더 단계 결정).
    pub pos: Option<(LogicalPx, LogicalPx)>,
    /// 사용자가 리사이즈한 크기. `None` = 기본 크기(렌더 단계 결정).
    pub size: Option<(LogicalPx, LogicalPx)>,
}

impl Default for ModifierHintSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            pos: None,
            size: None,
        }
    }
}

/// `~/.tasty/memory.db` 의 quota 정책. 모든 byte cap 은 MiB 단위 정수.
/// 0 또는 음수는 invalid 로 reject (음수는 serde 단계에서 unsigned 로 차단).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemorySettings {
    /// 단일 entry 의 value 최대 byte (MiB).
    pub entry_max_mb: u64,
    /// 각 plugin 의 secret 영역 (`owner` 별) 최대 byte (MiB).
    pub secret_quota_mb_per_plugin: u64,
    /// Regular 영역 전체 합산 최대 byte (MiB).
    pub regular_quota_mb_total: u64,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            entry_max_mb: 1,
            secret_quota_mb_per_plugin: 10,
            regular_quota_mb_total: 1024,
        }
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sound: false,
            coalesce_ms: 500,
        }
    }
}
