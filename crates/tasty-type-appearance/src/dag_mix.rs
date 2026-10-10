//! DAG 표시 색의 수기 접근자. design 의 DAG 상태·주기·러너 채움은 accent 를 surface-raised 나
//! bg-panel 에 섞는 color-mix 식이라 생성기가 건너뛴다. 디자인 JSON의 해당 식이 바뀌면 함께
//! 갱신해야 한다. transparent와 섞는 테두리는 알파만 낮춘다.

use crate::color::HexColor;
use crate::theme::{DAG_MIX_45_ALPHA, Theme, mix_srgb};

impl Theme {
    /// `component.dag-status-running-bg` = accent-primary 16% + surface-raised.
    #[inline]
    pub fn dag_status_running_bg(&self) -> HexColor {
        mix_srgb(self.accent_primary(), 0.16, self.surface_raised())
    }

    /// `component.dag-status-failed-bg` = accent-danger 12% + surface-raised.
    #[inline]
    pub fn dag_status_failed_bg(&self) -> HexColor {
        mix_srgb(self.accent_danger(), 0.12, self.surface_raised())
    }

    /// `component.dag-status-unknown-bg` = accent-warning 10% + surface-raised.
    #[inline]
    pub fn dag_status_unknown_bg(&self) -> HexColor {
        mix_srgb(self.accent_warning(), 0.10, self.surface_raised())
    }

    /// `component.dag-status-partially-failed-bg` = accent-attention 12% + surface-raised.
    #[inline]
    pub fn dag_status_partially_failed_bg(&self) -> HexColor {
        mix_srgb(self.accent_attention(), 0.12, self.surface_raised())
    }

    /// `component.dag-phase-awaiting-bg` = attention-needs-input 12% + surface-raised.
    #[inline]
    pub fn dag_phase_awaiting_bg(&self) -> HexColor {
        mix_srgb(self.attention_needs_input(), 0.12, self.surface_raised())
    }

    /// `component.dag-cycle-bg` = accent-warning 14% + bg-panel.
    #[inline]
    pub fn dag_cycle_bg(&self) -> HexColor {
        mix_srgb(self.accent_warning(), 0.14, self.bg_panel())
    }

    /// `component.dag-cycle-border` = accent-warning 45% + transparent.
    #[inline]
    pub fn dag_cycle_border(&self) -> HexColor {
        self.accent_warning().with_alpha(DAG_MIX_45_ALPHA)
    }

    /// `component.dag-runner-crashed-bg` = accent-danger 12% + surface-raised.
    #[inline]
    pub fn dag_runner_crashed_bg(&self) -> HexColor {
        mix_srgb(self.accent_danger(), 0.12, self.surface_raised())
    }

    /// `component.dag-runner-crashed-border` = accent-danger 45% + transparent.
    #[inline]
    pub fn dag_runner_crashed_border(&self) -> HexColor {
        self.accent_danger().with_alpha(DAG_MIX_45_ALPHA)
    }

    /// `component.dag-runner-stalled-bg` = accent-warning 10% + surface-raised.
    #[inline]
    pub fn dag_runner_stalled_bg(&self) -> HexColor {
        mix_srgb(self.accent_warning(), 0.10, self.surface_raised())
    }

    /// `component.dag-runner-stalled-border` = accent-warning 45% + transparent.
    #[inline]
    pub fn dag_runner_stalled_border(&self) -> HexColor {
        self.accent_warning().with_alpha(DAG_MIX_45_ALPHA)
    }
}
