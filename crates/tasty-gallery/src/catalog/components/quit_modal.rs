//! 종료 확인 창의 정적 예제. 본체는 close_behavior가 ask일 때 별도 창으로 띄운다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체 `open_quit_modal` 의 창 크기.
const WINDOW_W: LogicalPx = LogicalPx(400.0);
const WINDOW_H: LogicalPx = LogicalPx(200.0);

fn window(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(WINDOW_W.value(), WINDOW_H.value()),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, theme.corner_radius.value(), theme.bg_app().to_egui());
    ui.painter().rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );

    // 푸터 높이 — 본체 exact_height(52) = 28 + 12 + 12.
    let footer_h = theme.item_height_interactive.value() + theme.spacing_md.value() * 2.0;
    let body_rect =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - footer_h));
    let footer_rect = egui::Rect::from_min_max(egui::pos2(rect.min.x, body_rect.max.y), rect.max);

    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(body_rect));
    body.vertical_centered(|ui| {
        ui.add_space(theme.spacing_xl.value());
        ui.label(
            egui::RichText::new("Close Tasty")
                .size(theme.font_size_max.value())
                .strong()
                .color(theme.text_primary().to_egui()),
        );
        ui.add_space(theme.spacing_md.value());
        ui.label(
            egui::RichText::new("Would you like to quit or minimize to background?")
                .size(theme.font_size_body.value())
                .color(theme.text_primary().to_egui()),
        );
        ui.add_space(theme.spacing_sm.value());
        ui.label(
            egui::RichText::new("You can change the default behavior in Settings > General.")
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });

    let side = theme.spacing_lg.value();
    let gap = theme.spacing_sm.value();
    let inner_w = footer_rect.width() - side * 2.0;
    let button_w = (inner_w - gap) * 0.5;
    let btn_y = footer_rect.min.y + theme.spacing_md.value();
    let btn_h = theme.item_height_interactive.value();

    let mut place = |x: f32, label: &str, variant: ButtonVariant| {
        let slot = egui::Rect::from_min_size(egui::pos2(x, btn_y), egui::vec2(button_w, btn_h));
        let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(slot));
        Button::new(label)
            .variant(variant)
            .block(true)
            .show(&mut cell, theme);
    };
    place(footer_rect.min.x + side, "Quit", ButtonVariant::Primary);
    place(
        footer_rect.min.x + side + button_w + gap,
        "Minimize",
        ButtonVariant::Secondary,
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Quit confirmation", |ui| window(ui, theme));
    });

    spec::meta(
        ui,
        theme,
        &[
            ("window", "400×200 · non-resizable · 독립 winit 창"),
            (
                "body",
                "spacing-xl → 제목 → spacing-md → 안내 → spacing-sm → 힌트",
            ),
            (
                "footer",
                "높이 28+12×2 · 좌우 spacing-lg · 반반 · 사이 spacing-sm",
            ),
            ("hint", "font-size-caption text-muted"),
        ],
        &[
            TokenChip::new("bg-app", "window", theme.bg_app().to_egui()),
            TokenChip::new(
                "text-primary",
                "title · message",
                theme.text_primary().to_egui(),
            ),
            TokenChip::new("text-muted", "settings hint", theme.text_muted().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "`close_behavior = \"ask\"` 일 때만 뜬다. 이미 열려 있는 상태에서 다시 종료를 요청하면 \
         묻지 않고 즉시 종료한다 — 같은 확인을 두 번 쌓지 않는다. 두 버튼은 각각 종료와 \
         백그라운드 최소화로 갈리고, 기본 동작은 Settings › General 에서 바꾼다.",
    );
}
