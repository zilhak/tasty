//! 터미널 위 파일 드롭 안내의 정적 예제. 본체의 채움·테두리·라벨 순서를 따른다.
//! 갤러리는 주어진 Ui 안에 그리며 실제 오버레이 순서는 재현하지 않는다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체 `draw_drop_overlay` 가 쓰는 alpha 두 값 (0..255).
const FILL_ALPHA: u8 = 31;
const BORDER_ALPHA: u8 = 153;

/// 무대 한 칸 크기 — 터미널 rect 를 대신하는 데모 면적.
fn stage_size(theme: &Theme) -> egui::Vec2 {
    egui::vec2(theme.measure_md.value(), theme.measure_sm.value() * 0.5)
}

/// 터미널 rect 위 overlay 1장. `label` 은 단일/다중 파일 문구.
fn overlay(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    let (rect, _) = ui.allocate_exact_size(stage_size(theme), egui::Sense::hover());
    let p = ui.painter_at(rect);

    p.rect_filled(rect, theme.corner_radius.value(), theme.bg_app().to_egui());

    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.accent_primary().with_alpha(FILL_ALPHA).to_egui(),
    );

    p.rect_stroke(
        rect.shrink(theme.spacing_sm.value()),
        theme.corner_radius.value(),
        egui::Stroke::new(
            theme.border_width.value(),
            theme.accent_primary().with_alpha(BORDER_ALPHA).to_egui(),
        ),
        egui::StrokeKind::Inside,
    );

    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_heading.value()),
        theme.text_primary().to_egui(),
    );
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
            ("fill", "accent-primary @ 12% · corner-radius"),
            ("border", "1px accent-primary @ 60% · inset spacing-sm"),
            ("label", "font-size-heading · text-primary · centered"),
            ("layer", "Order::Tooltip — popup 위, plugin popup 아래"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "fill + border",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("text-primary", "label", theme.text_primary().to_egui()),
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
