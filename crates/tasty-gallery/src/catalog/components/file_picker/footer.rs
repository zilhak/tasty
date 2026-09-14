//! `file_picker` specimen 의 footer — 이름 행 · 덮어쓰기 경고 줄 · Cancel / primary.
//! 권위 원본: `gallery/overlays-shared.jsx` `FilePickerFrame` footer(`mode`/`save`).

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

use super::{
    CRUMB_GLYPH, FOOTER_CHIP_W, FOOTER_H, FOOTER_LABEL_W, FRAME_W, FpState, MULTI_PICKED, Mode,
    Variant,
};
use crate::catalog::icons;
use crate::catalog::widgets::dialog as kit;
use tasty_type_appearance::theme::Theme;

/// 덮어쓰기 경고 줄 한 줄의 높이 — caption 글꼴의 행 높이와 경고 글리프 중 큰 쪽.
fn warning_line_height(ui: &egui::Ui, theme: &Theme) -> LogicalPx {
    let row =
        ui.fonts(|f| f.row_height(&egui::FontId::proportional(theme.font_size_caption.value())));
    LogicalPx(row).max(CRUMB_GLYPH)
}

/// footer 높이. 덮어쓰기 상태면 경고 줄과 그 위 간격만큼 커진다.
pub(super) fn footer_height(ui: &egui::Ui, theme: &Theme, v: Variant) -> LogicalPx {
    if v.overwrite() {
        FOOTER_H + warning_line_height(ui, theme) + theme.spacing_sm
    } else {
        FOOTER_H
    }
}

pub(super) fn footer(ui: &mut egui::Ui, theme: &Theme, v: Variant, footer_h: LogicalPx) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(FRAME_W.value(), footer_h.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + theme.spacing_lg.value(),
            rect.top() + theme.spacing_sm.value(),
        ),
        egui::pos2(
            rect.right() - theme.spacing_lg.value(),
            rect.bottom() - theme.spacing_sm.value(),
        ),
    );
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing.y = theme.spacing_sm.value();

    let multi = v.multi;
    let (name_text, placeholder): (String, bool) = match (v.mode, v.state) {
        (Mode::Save(save), _) => (save.name().to_owned(), false),
        (Mode::Open, FpState::Loaded) if multi => (MULTI_PICKED.join(", "), false),
        (Mode::Open, FpState::Loaded) => ("README.md".to_owned(), false),
        _ => (String::new(), true),
    };
    let placeholder_text = match v.mode {
        Mode::Open => "No file selected",
        Mode::Save(_) => "Type a file name",
    };
    col.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        // 라벨·타입 필터 칩은 flex:none — 이름 칸(flex:1; min-width:0)만 준다.
        ui.allocate_ui_with_layout(
            egui::vec2(FOOTER_LABEL_W.value(), 0.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new("File name")
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            },
        );
        let remaining = ui.available_width();
        let input_w = (LogicalPx(remaining) - FOOTER_CHIP_W - theme.spacing_sm).max(LogicalPx(0.0));
        kit::field(
            ui,
            theme,
            Some(input_w),
            if placeholder {
                placeholder_text
            } else {
                name_text.as_str()
            },
            placeholder,
            false,
        );
        type_filter_chip(ui, theme);
    });

    if v.overwrite() {
        overwrite_line(&mut col, theme, &name_text);
    }

    col.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        if multi {
            ui.label(
                egui::RichText::new(format!("{} selected", MULTI_PICKED.len()))
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, enabled) = match v.mode {
                Mode::Open => ("Open", v.state == FpState::Loaded),
                Mode::Save(_) if v.overwrite() => ("Overwrite", !name_text.is_empty()),
                Mode::Save(_) => ("Save", !name_text.is_empty()),
            };
            Button::new(label)
                .variant(ButtonVariant::Primary)
                .enabled(enabled)
                .show(ui, theme);
            Button::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .show(ui, theme);
        });
    });
}

/// 덮어쓰기 경고 줄 — `alertTriangle` + "`<name>` already exists in this folder. Saving
/// replaces it." (11px · `accent-warning`, 이름만 mono).
fn overwrite_line(ui: &mut egui::Ui, theme: &Theme, name: &str) {
    let warn = theme.accent_warning().to_egui();
    let size = theme.font_size_caption.value();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
        kit::icon(ui, icons::ALERT_TRIANGLE, CRUMB_GLYPH, warn);
        let mut job = egui::text::LayoutJob::default();
        job.append(
            name,
            0.0,
            egui::TextFormat::simple(egui::FontId::monospace(size), warn),
        );
        job.append(
            " already exists in this folder. Saving replaces it.",
            0.0,
            egui::TextFormat::simple(egui::FontId::proportional(size), warn),
        );
        ui.label(job);
    });
}

/// "All files ▾" 타입 필터 칩 — 정적(팝오버 미열림) specimen.
fn type_filter_chip(ui: &mut egui::Ui, theme: &Theme) {
    let h = theme.item_height_interactive.value();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(FOOTER_CHIP_W.value(), h), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.bg_panel().to_egui(),
    );
    ui.painter().rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
    let pad = theme.spacing_sm.value();
    ui.painter().text(
        egui::pos2(rect.left() + pad, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "All files",
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_secondary().to_egui(),
    );
    let ir = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - pad - CRUMB_GLYPH.value(),
            rect.center().y - CRUMB_GLYPH.value() * 0.5,
        ),
        egui::vec2(CRUMB_GLYPH.value(), CRUMB_GLYPH.value()),
    );
    icons::CHEVRON_DOWN
        .image(CRUMB_GLYPH.value(), theme.text_muted().to_egui())
        .paint_at(ui, ir);
}
