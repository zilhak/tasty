//! `file_picker` specimen 의 footer — 이름 행 · 덮어쓰기 경고 줄 · Cancel / primary.
//! 권위 원본: `gallery/overlays-shared.jsx` `FilePickerFrame` footer(`mode`/`save`).

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

use super::{
    CRUMB_GLYPH, FOLDER_SEL, FOOTER_CHIP_W, FOOTER_H, FOOTER_LABEL_W, FRAME_W, FpState,
    MULTI_PICKED, Mode, Variant,
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

/// footer 높이. 덮어쓰기 줄과 폴더 안내 줄은 서로 배타적이지 않지만, 한 줄이 늘 때마다
/// 그 줄과 그 위 간격만큼 커진다.
pub(super) fn footer_height(ui: &egui::Ui, theme: &Theme, v: Variant) -> LogicalPx {
    let mut h = FOOTER_H;
    for _ in 0..usize::from(v.overwrite()) + usize::from(v.folder_sel) {
        h = h + warning_line_height(ui, theme) + theme.spacing_sm;
    }
    h
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
    // 열기 모드에서 고른 것이 폴더면 이름 칸은 비어 있다 — 확정 버튼이 읽는 값은 파일 이름이고,
    // 폴더는 그 값이 될 수 없다. 저장 모드는 이름 칸이 목록과 독립이라 그대로 둔다.
    let (name_text, placeholder): (String, bool) = match (v.mode, v.state) {
        (Mode::Save(save), _) => (save.name().to_owned(), false),
        (Mode::Open, _) if v.folder_sel => (String::new(), true),
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

    if v.folder_sel {
        folder_line(&mut col, theme, v.mode);
    }
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

/// 폴더를 고른 상태의 안내 줄 — `folder` 글리프 + muted caption. 경고가 아니라 **읽는
/// 자리에 붙는 사실**이라 톤이 없다: 저장은 그것이 대상이 될 수 없다고 말하고, 열기는
/// 확정하면 들어간다고 말한다. 강조는 굵기가 아니라 색으로 준다(egui 관례).
fn folder_line(ui: &mut egui::Ui, theme: &Theme, mode: Mode) {
    let muted = theme.text_muted().to_egui();
    let size = theme.font_size_caption.value();
    let prose = egui::TextFormat::simple(egui::FontId::proportional(size), muted);
    let mono = egui::TextFormat::simple(egui::FontId::monospace(size), muted);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
        kit::icon(ui, icons::FOLDER, CRUMB_GLYPH, muted);
        let mut job = egui::text::LayoutJob::default();
        match mode {
            Mode::Save(_) => {
                job.append(
                    "Folders aren't save targets — double-click ",
                    0.0,
                    prose.clone(),
                );
                job.append(FOLDER_SEL, 0.0, mono);
                job.append(" to open it.", 0.0, prose);
            }
            Mode::Open => {
                job.append(FOLDER_SEL, 0.0, mono);
                job.append(" is a folder — ", 0.0, prose.clone());
                job.append(
                    "Open",
                    0.0,
                    egui::TextFormat::simple(
                        egui::FontId::proportional(size),
                        theme.text_secondary().to_egui(),
                    ),
                );
                job.append(" enters it.", 0.0, prose);
            }
        }
        ui.label(job);
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
