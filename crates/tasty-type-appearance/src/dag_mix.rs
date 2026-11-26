//! DAG 표시 색의 수기 접근자. design 의 DAG 상태·block·주기·러너 채움은 accent 를 surface-raised 나
//! bg-panel 에 섞는 color-mix 식이라 생성기가 건너뛴다. 디자인 JSON의 해당 식이 바뀌면 함께
//! 갱신해야 한다. transparent와 섞는 테두리는 알파만 낮춘다.
//! block 채움(`dag-*-block-bg`)은 반투명 강조색에서도 정본 color-mix의 알파 규칙을 따르도록
//! `mix_srgb_premultiplied`로 섞는다.

use crate::color::HexColor;
use crate::theme::{DAG_MIX_45_ALPHA, Theme, mix_srgb, mix_srgb_premultiplied};

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

    /// `component.dag-status-waiting-block-bg` = glyph-dim-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_waiting_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.glyph_dim_raised(), 55, self.surface_raised())
    }

    /// `component.dag-status-ready-block-bg` = accent-info-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_ready_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_info_raised(), 55, self.surface_raised())
    }

    /// `component.dag-status-running-block-bg` = accent-primary-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_running_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_primary_raised(), 55, self.surface_raised())
    }

    /// `component.dag-status-succeeded-block-bg` = accent-success-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_succeeded_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_success_raised(), 55, self.surface_raised())
    }

    /// `component.dag-status-failed-block-bg` = accent-danger-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_failed_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_danger_raised(), 55, self.surface_raised())
    }

    /// `component.dag-status-cancelled-block-bg` = glyph-dim-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_cancelled_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.glyph_dim_raised(), 55, self.surface_raised())
    }

    /// `component.dag-status-skipped-block-bg` = glyph-dim-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_skipped_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.glyph_dim_raised(), 55, self.surface_raised())
    }

    /// `component.dag-status-unknown-block-bg` = accent-warning-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_unknown_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_warning_raised(), 55, self.surface_raised())
    }

    /// `component.dag-status-partially-failed-block-bg` = accent-attention-raised 55% + surface-raised.
    #[inline]
    pub fn dag_status_partially_failed_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_attention_raised(), 55, self.surface_raised())
    }

    /// `component.dag-phase-awaiting-block-bg` = accent-warning-strong 55% + surface-raised.
    #[inline]
    pub fn dag_phase_awaiting_block_bg(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_warning_strong(), 55, self.surface_raised())
    }
}

#[cfg(test)]
mod tests {
    use crate::state_ink::CSS_TRANSPARENT;
    use crate::theme::mix_srgb_premultiplied;

    /// 투명한 앞 색만 100% 섞으면 섞은 알파가 0이라, 불투명한 뒤 색이 아니라 투명 검정이다.
    #[test]
    fn a_full_ratio_of_a_clear_color_is_transparent_black() {
        let clear = crate::hex!("#89dceb00");
        let ground = crate::hex!("#313244");
        assert_eq!(mix_srgb_premultiplied(clear, 100, ground), CSS_TRANSPARENT);
        assert_eq!(mix_srgb_premultiplied(clear, 100, ground).a(), 0);
    }
}
