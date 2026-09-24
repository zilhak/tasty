//! 빈 서피스의 타입 선택 버튼 예제. 갤러리는 공용 Button을 사용한다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// surface 무대 1칸 — 본체의 pane 본문에 대응.
fn surface_body(ui: &mut egui::Ui, theme: &Theme) {
    let size = egui::vec2(theme.measure_md.value(), theme.measure_sm.value() * 0.6);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_app().to_egui());

    // 상위 레이아웃과 무관하게 세로 간격을 적용하려고 방향을 명시한다.
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let btn_h = theme.item_height_interactive.value();
    child.add_space(((rect.height() - btn_h) * 0.5).max(0.0));
    child.vertical_centered(|ui| {
        Button::new("Surface Type")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Empty surface", |ui| surface_body(ui, theme));
    });

    spec::meta(
        ui,
        theme,
        &[
            ("background", "bg-app, 전면 · radius 0"),
            ("button", "item-height-interactive(28) · 세로 중앙"),
            ("action", "convert popup 을 이 surface 대상으로 연다"),
        ],
        &[
            TokenChip::new("bg-app", "surface body", theme.bg_app().to_egui()),
            TokenChip::new(
                "text-primary",
                "button label",
                theme.text_primary().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "빈 서피스에는 타입 선택 버튼을 표시한다. 분할로 새로 만든 서피스와 타입을 비운 \
         서피스가 같은 화면을 사용한다.",
    );
}
