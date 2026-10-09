//! 터미널 위 파일 드롭 안내의 정적 예제. 본체와 같은 `tasty_ui_widgets::file_drop_overlay` 로 그린다.
//! 갤러리는 주어진 Ui 안에 그리며 실제 오버레이 순서는 재현하지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::file_drop_overlay;

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 무대 한 칸 크기 — 터미널 rect 를 대신하는 데모 면적.
fn stage_size(theme: &Theme) -> egui::Vec2 {
    egui::vec2(theme.measure_md.value(), theme.measure_sm.value() * 0.5)
}

/// 터미널 rect 위 overlay 1장. `label` 은 단일/다중 파일 문구.
fn overlay(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    let (rect, _) = ui.allocate_exact_size(stage_size(theme), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, theme.corner_radius.value(), theme.bg_app().to_egui());
    file_drop_overlay(ui, theme, rect, label);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Single file", |ui| {
            overlay(ui, theme, "Drop to open")
        });
        spec::cluster(ui, theme, "Multiple files", |ui| {
            overlay(ui, theme, "Drop to open  (3 files)")
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "box",
                "inset spacing-sm · file-drop-overlay-bg (× tint-fill-alpha 0.12) · 1px file-drop-overlay-border (× tint-border-alpha 0.36) · corner-radius",
            ),
            (
                "content",
                "download glyph icon-size-md + spacing-sm + label font-size-body · file-drop-overlay-fg (full ink) · centered",
            ),
            ("layer", "Order::Tooltip — popup 위, plugin popup 아래"),
        ],
        &[
            TokenChip::new(
                "file-drop-overlay-bg",
                "fill",
                theme.file_drop_overlay_bg().to_egui(),
            ),
            TokenChip::new(
                "file-drop-overlay-border",
                "border",
                theme.file_drop_overlay_border().to_egui(),
            ),
            TokenChip::new(
                "file-drop-overlay-fg",
                "label",
                theme.file_drop_overlay_fg().to_egui(),
            ),
            TokenChip::new("bg-app", "terminal beneath", theme.bg_app().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "터미널 rect 위에만 뜬다 — 그 밖에 떨어진 파일은 무시되고 별도 안내가 나간다. \
         hover 가 취소되거나 파일이 실제로 떨어지면 다음 프레임에 사라진다(지속 상태 없음).",
    );
}
