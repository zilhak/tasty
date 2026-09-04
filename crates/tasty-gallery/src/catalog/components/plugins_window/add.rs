//! `Add plugin` 탭 specimen — 본체 `src/view/plugins/ui/add.rs` 의 구조 전사.
//!
//! 본체는 상태가 둘이고 같은 `CentralPanel` 을 갈아 끼운다.
//!
//! - **경로 입력**(`draw_add_input`) — 라벨 → 입력 + Verify → 구분선 → 폴더 찾기.
//! - **매니페스트 프리뷰**(`draw_add_preview`) — 제목 → 이름/버전 → id → 설명 →
//!   authors/homepage → source → surface kinds → permissions → (미신뢰면)
//!   경고 영역 → 구분선 → Add / Cancel.
//!
//! 두 상태를 나란히 전시한다 — 한 화면에 하나만 나오는 것은 본체 동작이고,
//! 갤러리는 두 상태가 다 있는 것을 보여야 한다.
//!
//! **미신뢰 경고**(`draw_untrusted_warning`)는 trust 상태로 갈린다. 여기서는
//! 공개키가 있어 신뢰 등록이 가능한 경우(`UntrustedWithPubkey`)를 그린다 —
//! 그 경우만 Add 버튼이 살아 있어 버튼 활성 규칙까지 함께 보인다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant};

/// 경로 입력은 남은 폭을 다 먹고 옆 `Verify` 자리만 비운다 — 본체
/// `desired_width(ui.available_width() - 90.0)`. 그 90 은 토큰과 같은 값이라
/// (`field_width_xs`) 갤러리는 raw 숫자 대신 토큰으로 같은 폭을 만든다.
fn field_width(theme: &Theme, available: f32) -> f32 {
    (available - theme.field_width_xs.value()).max(theme.field_width_xs.value())
}

/// 경로 입력 상태.
pub(super) fn input_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let inner = rect.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.spacing_mut().item_spacing.y = theme.spacing_sm.value();

    child.label(
        egui::RichText::new("Plugin folder path")
            .size(theme.font_size_body.value())
            .color(theme.text_primary().to_egui()),
    );
    child.horizontal(|ui| {
        let h = theme.item_height_interactive.value();
        let w = field_width(theme, ui.available_width());
        let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter().rect(
            r,
            theme.corner_radius.value(),
            theme.surface_raised().to_egui(),
            egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            egui::pos2(r.min.x + theme.spacing_sm.value(), r.center().y),
            egui::Align2::LEFT_CENTER,
            "/path/to/plugin/directory",
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_placeholder().to_egui(),
        );
        Button::new("Verify")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
    });
    child.separator();
    Button::new("Find plugin folder…")
        .variant(ButtonVariant::Secondary)
        .show(&mut child, theme);
}

/// 한 줄짜리 라벨-값 행 — 본체 프리뷰의 `label(format!("{}: {}"))` 들.
fn field(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
        ui.label(
            egui::RichText::new(value)
                .size(theme.font_size_caption.value())
                .color(theme.text_secondary().to_egui()),
        );
    });
}

/// 매니페스트 프리뷰 상태 (미신뢰 · 공개키 있음).
pub(super) fn preview_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let inner = rect.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.spacing_mut().item_spacing.y = theme.spacing_xs.value();

    child.label(
        egui::RichText::new("Plugin information")
            .size(theme.font_size_max.value())
            .strong()
            .color(theme.text_primary().to_egui()),
    );
    child.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Port scanner")
                .size(theme.font_size_max.value())
                .color(theme.text_primary().to_egui()),
        );
        ui.label(
            egui::RichText::new("v0.2.0")
                .size(theme.font_size_body.value())
                .color(theme.text_secondary().to_egui()),
        );
    });
    child.label(
        egui::RichText::new("com.example.port-scanner")
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
    child.label(
        egui::RichText::new("Scans listening ports and shows what owns them.")
            .size(theme.font_size_body.value())
            .color(theme.text_primary().to_egui()),
    );
    field(&mut child, theme, "Authors:", "example");
    field(&mut child, theme, "Source:", "~/dev/port-scanner");
    field(&mut child, theme, "Surface kinds:", "port-scanner");
    field(&mut child, theme, "Permissions:", "net · process:read");

    untrusted_warning(&mut child, theme);

    child.separator();
    child.horizontal(|ui| {
        Button::new("Add")
            .variant(ButtonVariant::Primary)
            .show(ui, theme);
        Button::new("Cancel")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
    });
}

/// 출처 미상 경고 — 본체 `draw_untrusted_warning` 의 `UntrustedWithPubkey` 가지.
fn untrusted_warning(ui: &mut egui::Ui, theme: &Theme) {
    let red = theme.accent_danger().to_egui();
    ui.separator();
    ui.label(
        egui::RichText::new("Unknown source plugin")
            .strong()
            .size(theme.font_size_body.value())
            .color(red),
    );
    ui.label(
        egui::RichText::new(
            "This plugin is not signed by a verified key. Adding it will permanently record \
             your trust so future loads are automatic.",
        )
        .size(theme.font_size_caption.value())
        .color(theme.text_primary().to_egui()),
    );
    ui.label(
        egui::RichText::new("Fingerprint: SHA256:9f2c…a17e")
            .monospace()
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}
