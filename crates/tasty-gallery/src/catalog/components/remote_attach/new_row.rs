//! New-workspace row states and their specimen strips.

use super::rows::{dot_slot, selected_bar, ws_row};
use super::{CREATE_ERROR, NewRow, STRIP_W, WORKSPACES, WS_ROW_H};
use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, Spinner};

/// "+ New workspace" 행 5상태 — 440px pane 폭 스트립(디자인 specimen 과 동일 폭).
pub fn draw_new_row(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for (label, state) in [
            ("rest", NewRow::Rest),
            ("hover", NewRow::Hover),
            ("selected", NewRow::Selected),
            ("creating", NewRow::Creating),
            ("failed", NewRow::Failed),
        ] {
            spec::cluster(ui, theme, label, |ui| {
                new_row_strip(ui, theme, state);
            });
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("box", "34px — the same row as a remote workspace"),
            (
                "glyph",
                "plus 14px centered in a status-dot-width slot (overflows both sides)",
            ),
            ("label", "13px / 500 — reads as a peer of the rows below"),
            (
                "rest · hover",
                "accent-primary label on panel / overlay-hover",
            ),
            (
                "selected",
                "text-primary on surface-active + 2px accent bar",
            ),
            (
                "creating",
                "Spinner + muted 'Creating workspace…'; list dims",
            ),
            ("failed", "danger warn glyph + inline reason + Try again"),
            ("group", "1px separator below, xs margin above and below"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "rest/hover glyph + label",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "overlay-hover",
                "hover fill",
                theme.overlay_hover().to_egui(),
            ),
            TokenChip::new(
                "surface-active",
                "selected fill",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "failed glyph + reason",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "text-muted",
                "'on remote' caption / creating label",
                theme.text_muted().to_egui(),
            ),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Keep the glyph inside a status-dot-width slot. The dot the workspace rows draw is \
         8px and the plus is 14px, so the plus overflows its slot symmetrically and the name \
         column starts on exactly the same pixel as every row beneath it.",
    );

    spec::note(
        ui,
        theme,
        "Selected drops the accent label for text-primary. Accent on the active surface \
         measures 3.17:1 — the row would be least readable at the moment it is chosen. The \
         row still reads as the odd one out through the glyph, the separator, and the accent \
         bar, so nothing is lost by letting the label go quiet.",
    );

    spec::note(
        ui,
        theme,
        "Creating dims the list below it rather than replacing the pane with a spinner — the \
         round trip is a second or two and the user was reading that list. Failure lands \
         under the row for the same reason: after a failed create the next move is usually \
         to pick an existing workspace, so the list has to stay on screen. The remote's \
         message can be long; it clamps to three lines and carries the rest in a tooltip.",
    );
}

// ════════════════════════════════════════════════════════════════════════
/// 새 행 한 상태를 실제 pane 폭(440px)에서 보여주는 스트립 — 아래에 ws 행 하나를
/// 같이 깔아 두 행의 좌측 정렬선이 픽셀 동일한지 눈으로 확인할 수 있게 한다.
///
/// 이 스트립은 popup **안**의 목록 한 조각을 떼어 보이는 것이라 떠 있는 표면이 아니다
/// — SCOPE RULE(ADR-0254)의 세 번째 갈래로 lift 를 얹지 않는다(`ra_card` 와 다르다).
fn new_row_strip(ui: &mut egui::Ui, theme: &Theme, state: NewRow) {
    let peek = &WORKSPACES[0];
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_width(STRIP_W.value());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.vertical(|ui| {
                ui.set_width(STRIP_W.value());
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                new_ws_row(ui, theme, state);
                // 생성 중에는 아래 목록이 dim + inert 된다.
                let dim = if state.creating() { 0.5 } else { 1.0 };
                ui.scope(|ui| {
                    ui.set_opacity(dim);
                    ws_row(ui, theme, peek, false);
                });
            });
        });
}

/// "+ New workspace" — loaded 목록의 첫 행. ws 행과 같은 34px 박스이고, 실제 원격
/// 워크스페이스와는 **세 채널 동시**로 구분된다(글리프 · accent 라벨 · 아래 구분선).
/// 색 하나로만 구분하지 않는다.
pub(super) fn new_ws_row(ui: &mut egui::Ui, theme: &Theme, state: NewRow) {
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, WS_ROW_H.value()), egui::Sense::hover());
    if state.selected() {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        selected_bar(ui, theme, rect);
    } else if state == NewRow::Hover {
        ui.painter()
            .rect_filled(rect, 0.0, theme.overlay_hover().to_egui());
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + theme.spacing_md.value(), rect.top()),
        egui::pos2(rect.right() - theme.spacing_md.value(), rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    let glyph_c = if state.creating() {
        theme.text_muted()
    } else if state.failed() {
        theme.accent_danger()
    } else {
        theme.accent_primary()
    };
    dot_slot_glyph(&mut child, theme, state, glyph_c.to_egui());
    // selected 에서만 accent 를 놓는다 — surface-active 위의 accent 는 3.17:1 이라
    // 고른 순간 가장 안 읽힌다. 구분은 글리프·구분선·accent 바가 계속 진다.
    let label_c = if state.creating() {
        theme.text_muted()
    } else if state.selected() {
        theme.text_primary()
    } else {
        theme.accent_primary()
    };
    child.label(
        egui::RichText::new(if state.creating() {
            "Creating workspace…"
        } else {
            "New workspace"
        })
        .size(theme.font_size_body.value())
        .strong()
        .color(label_c.to_egui()),
    );
    // 우측 슬롯 — status dot·pane 수·배지는 의미상 없는 행이라 캡션 하나뿐.
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if !state.creating() && !state.failed() {
            ui.label(
                egui::RichText::new("on remote")
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        }
    });
    if state.failed() {
        new_ws_error(ui, theme);
    }
    // 행 아래 1px 구분선 — 새 행 그룹을 닫는다.
    row_separator(ui, theme);
}

/// 새 행의 글리프 — 슬롯 중심에 놓인 14px `plus`(실패 시 `alertTriangle`,
/// 생성 중이면 Spinner).
fn dot_slot_glyph(ui: &mut egui::Ui, theme: &Theme, state: NewRow, color: egui::Color32) {
    let size = theme.icon_glyph_size_sm.value();
    let g = egui::Rect::from_center_size(dot_slot(ui, theme).center(), egui::vec2(size, size));
    if state.creating() {
        let mut c = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(g)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        Spinner::new().size(size).color(color).show(&mut c, theme);
    } else {
        let glyph = if state.failed() {
            icons::ALERT_TRIANGLE
        } else {
            icons::PLUS
        };
        glyph.image(size, color).paint_at(ui, g);
    }
}

/// 생성 실패 — 행 하단 인라인. connect-error center-state 는 "목록 자체를 못 받은"
/// 경우의 어휘이고, 여기서는 목록을 이미 쥐고 있으므로 가리지 않는다.
fn new_ws_error(ui: &mut egui::Ui, theme: &Theme) {
    let width = ui.available_width();
    let cap_h = theme.font_size_caption.value() * theme.line_height_ui;
    let btn_h = ControlSize::Sm.height(theme);
    let h = theme.spacing_xs.value() * 2.0 + cap_h + btn_h + theme.spacing_sm.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left()
                + theme.spacing_md.value()
                + theme.status_dot_size().value()
                + theme.spacing_sm.value(),
            rect.top() + theme.spacing_xs.value(),
        ),
        egui::pos2(
            rect.right() - theme.spacing_md.value(),
            rect.bottom() - theme.spacing_sm.value(),
        ),
    );
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing.y = theme.spacing_xs.value();
    col.label(
        egui::RichText::new(CREATE_ERROR)
            .size(theme.font_size_caption.value())
            .color(theme.accent_danger().to_egui()),
    );
    Button::new("Try again")
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
        .show(&mut col, theme);
}

/// 행 아래 1px 구분선 + 위/아래 xs 마진.
fn row_separator(ui: &mut egui::Ui, theme: &Theme) {
    let width = ui.available_width();
    let m = theme.spacing_xs.value();
    let t = theme.border_width.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, m * 2.0 + t), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + m + t * 0.5,
        egui::Stroke::new(t, theme.separator.to_egui()),
    );
}
