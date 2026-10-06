//! 탭 스트립과 세그먼트의 활성 표시 규칙 — 밑줄은 화면 전환, 채움은 값 선택.
//! 시안 `overlays-windows.jsx`의 "Segmented active is an accent fill" Spec을 옮긴다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::{attach_form_card, attach_frame};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 시안 무대의 두 예제 사이 간격(`gap: 18`)과 캡션 아래 간격(`gap: 6`).
const STAGE_GAP: LogicalPx = LogicalPx(18.0);
const CAPTION_GAP: LogicalPx = LogicalPx(6.0);

/// Overlays › Remote connections — 탭 스트립(밑줄)과 세그먼트(accent 채움)를 나란히 둔다.
pub fn draw_segment_rule(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(STAGE_GAP.value(), STAGE_GAP.value());
        // 시안 무대는 `alignItems: flex-start` 라 높이가 다른 두 창도 위쪽을 맞춘다.
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
            |ui| {
                specimen(ui, theme, "tab strip — underline", |ui| {
                    attach_frame(ui, theme)
                });
                specimen(ui, theme, "segmented — accent fill", |ui| {
                    attach_form_card(ui, theme, false)
                });
            },
        );
    });
    spec::meta(
        ui,
        theme,
        &[
            ("tab strip", "2px accent-primary underline · weight 600"),
            ("segmented", "fill accent-primary · ink text-on-accent"),
            ("inactive segment", "surface-raised"),
            (
                "scope",
                "every remote segment + clipboard type + preset scope",
            ),
            ("not used", "surface-active (row selection)"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "active segment / underline",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "text-on-accent",
                "active segment ink",
                theme.text_on_accent().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "inactive segment",
                theme.surface_raised().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Rule of thumb: an underline switches a view, a fill switches a value. The remote tab bar \
         navigates; the attach switch sets a field.",
    );
}

fn specimen(ui: &mut egui::Ui, theme: &Theme, caption: &str, add: impl FnOnce(&mut egui::Ui)) {
    spec::wrap_item(ui, |ui| {
        ui.push_id(caption, |ui| {
            ui.spacing_mut().item_spacing.y = CAPTION_GAP.value();
            ui.label(
                egui::RichText::new(caption)
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
            add(ui);
        });
    });
}
