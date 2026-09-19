//! Profile/workspace rows and their shared selection and dot-slot geometry.

use super::{BADGE_H, PROFILE_ROW_H, Prof, WS_ROW_H, Ws};
use crate::catalog::icons;
use crate::catalog::widgets::dialog as kit;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{StatusKind, status_dot};

pub(super) fn profile_row(ui: &mut egui::Ui, theme: &Theme, p: &Prof, selected: bool) {
    let w = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(w, PROFILE_ROW_H.value()), egui::Sense::hover());
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        selected_bar(ui, theme, rect);
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + theme.spacing_md.value(),
            rect.top() + theme.spacing_sm.value(),
        ),
        egui::pos2(
            rect.right() - theme.spacing_md.value(),
            rect.bottom() - theme.spacing_sm.value(),
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.spacing_mut().item_spacing.y = STRUCT_GAP_2.value();
    child.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        let name_c = if selected {
            theme.text_primary()
        } else {
            theme.text_secondary()
        };
        ui.label(
            egui::RichText::new(p.name)
                .size(theme.font_size_body.value())
                .strong()
                .color(name_c.to_egui()),
        );
        if !p.label.is_empty() {
            ui.label(
                egui::RichText::new(format!("({})", p.label))
                    .size(theme.font_size_body.value())
                    .color(theme.text_muted().to_egui()),
            );
        }
        if p.inactive {
            badge(
                ui,
                theme,
                "inactive",
                theme.accent_warning().to_egui(),
                0.12,
                0.40,
                true,
            );
        }
    });
    child.label(
        egui::RichText::new(p.target)
            .monospace()
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 원격이 닿기는 하는데 ws 가 없을 때 새 행 아래 붙는 muted 한 줄. 이름 열은 위
/// 행들과 같은 정렬선에서 시작한다(선행 dot 슬롯 폭 스페이서).
pub(super) fn empty_line(ui: &mut egui::Ui, theme: &Theme, profile: &str) {
    let width = ui.available_width();
    let h = theme.spacing_xs.value() * 2.0 + theme.font_size_caption.value() * theme.line_height_ui;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let x = rect.left()
        + theme.spacing_md.value()
        + theme.status_dot_size().value()
        + theme.spacing_sm.value();
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{profile} is reachable but has no workspaces yet."),
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
}

/// 이름 열 앞의 status-dot 슬롯(8px)을 할당한다. 목록의 **모든** 행이 이 한 함수로
/// 슬롯을 잡으므로 이름 열의 좌측 정렬선이 픽셀 동일해진다 — 새 행의 14px 글리프는
/// 슬롯보다 넓지만 좌우로 대칭 overflow 하므로 정렬선을 밀지 않는다.
pub(super) fn dot_slot(ui: &mut egui::Ui, theme: &Theme) -> egui::Rect {
    let (slot, _) = ui.allocate_exact_size(
        egui::vec2(
            theme.status_dot_size().value(),
            theme.icon_glyph_size_sm.value(),
        ),
        egui::Sense::hover(),
    );
    slot
}

/// ws 행의 실행 dot — 같은 슬롯 안에 그린다. `status_dot` 은 라벨이 비어도 dot 뒤에
/// 자기 gap 을 할당하므로 그대로 부르면 이름 열이 새 행보다 밀린다. 슬롯을 먼저
/// 잡고 그 안의 child 에 그려서, 위젯이 삼키는 여백이 정렬선에 새지 않게 한다.
fn dot_slot_status(ui: &mut egui::Ui, theme: &Theme, kind: StatusKind, pulse: bool) {
    let slot = dot_slot(ui, theme);
    let mut c = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(slot)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    status_dot(&mut c, theme, kind, "", pulse, false);
}

/// 선택 행의 inset accent 좌측바 — listctrl 과 같은 2px 토큰.
pub(super) fn selected_bar(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let bar = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(theme.listctrl_selected_bar_width().value(), rect.height()),
    );
    ui.painter()
        .rect_filled(bar, 0.0, theme.listctrl_selected_bar().to_egui());
}

pub(super) fn ws_row(ui: &mut egui::Ui, theme: &Theme, w: &Ws, selected: bool) {
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, WS_ROW_H.value()), egui::Sense::hover());
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        selected_bar(ui, theme, rect);
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
    let kind = if w.busy {
        StatusKind::Running
    } else {
        StatusKind::Idle
    };
    dot_slot_status(&mut child, theme, kind, w.busy);
    let name_c = if w.attached {
        theme.text_disabled()
    } else if selected {
        theme.text_primary()
    } else {
        theme.text_secondary()
    };
    child.label(
        egui::RichText::new(w.name)
            .size(theme.font_size_body.value())
            .color(name_c.to_egui()),
    );
    // panes 아이콘 + count.
    kit::icon(
        &mut child,
        icons::SPLIT,
        theme.font_size_caption,
        theme.text_muted().to_egui(),
    );
    child.label(
        egui::RichText::new(w.panes.to_string())
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if w.attached {
            badge(
                ui,
                theme,
                "in use",
                theme.border_attached().to_egui(),
                // 본체와 같은 tint 짝 토큰.
                theme.tint_fill_alpha(),
                theme.tint_border_alpha(),
                false,
            );
        } else if w.busy {
            ui.label(
                egui::RichText::new("busy")
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        }
    });
}

/// 원격/inactive pill — fill/border alpha 는 디자인 color-mix(% transparent) 근사.
fn badge(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &str,
    color: egui::Color32,
    fill_a: f32,
    border_a: f32,
    warn_icon: bool,
) {
    let font = egui::FontId::monospace(theme.font_size_micro.value());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER);
    let pad_x = theme.spacing_sm.value();
    // 경고 글리프는 아이콘 스케일 xs(12), 글리프↔라벨 간격은 spacing_xs(4).
    let warn_glyph = theme.icon_glyph_size_xs.value();
    let warn_gap = theme.spacing_xs.value();
    let icon_w = if warn_icon {
        warn_glyph + warn_gap
    } else {
        0.0
    };
    let w = pad_x * 2.0 + icon_w + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, BADGE_H.value()), egui::Sense::hover());
    let radius = theme.corner_radius_sm.value();
    ui.painter()
        .rect_filled(rect, radius, color.gamma_multiply(fill_a));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), color.gamma_multiply(border_a)),
        egui::StrokeKind::Inside,
    );
    let mut tx = rect.left() + pad_x;
    if warn_icon {
        let ir = egui::Rect::from_min_size(
            egui::pos2(tx, rect.center().y - warn_glyph * 0.5),
            egui::vec2(warn_glyph, warn_glyph),
        );
        icons::ALERT_TRIANGLE
            .image(warn_glyph, color)
            .paint_at(ui, ir);
        tx += warn_glyph + warn_gap;
    }
    ui.painter().galley(
        egui::pos2(tx, rect.center().y - galley.rect.height() * 0.5),
        galley,
        color,
    );
}
