//! 원격 파일 전송의 진행·실패 팝업 예제.
//! 진행 막대는 비율을 바로 반영하며 애니메이션을 추가하지 않는다.
//! 실제 전송이나 배경 어둡게 하기는 실행하지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::TRANSFER_CARD_PAD_X;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant};
use crate::catalog::widgets::dialog as kit;

/// `--tasty-transfer-popup-width` (size-400).
const FRAME_W: LogicalPx = LogicalPx(400.0);
/// 헤더/푸터 가로 패딩 (디자인 14 — space 스텝 밖 raw).
const PAD_X: LogicalPx = LogicalPx(14.0);
/// 헤더 세로 패딩 (디자인 12 = space-md).
const HEADER_PAD_Y: LogicalPx = LogicalPx(12.0);
/// 바디 패딩 (디자인 14 — raw).
const BODY_PAD: LogicalPx = LogicalPx(14.0);
/// 푸터 세로 패딩 (디자인 10 — raw).
const FOOTER_PAD_Y: LogicalPx = LogicalPx(10.0);
/// 바디 내부 요소 gap (디자인 10 — raw).
const BODY_GAP: LogicalPx = LogicalPx(10.0);

/// 단일·다중 파일 전송의 진행 상태를 비교한다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "receiving (mid-transfer)", |ui| {
            progress_card(
                ui,
                theme,
                &[ProgressRow {
                    name: "sprint-42-demo.mp4",
                    pct: 27,
                    done: "34.6 MiB",
                    total: "128.0 MiB",
                    rate: "2.1 MiB/s",
                }],
            );
        });
        spec::cluster(ui, theme, "multiple files (row repeat)", |ui| {
            progress_card(
                ui,
                theme,
                &[
                    ProgressRow {
                        name: "clip-0001.png",
                        pct: 100,
                        done: "1.2 MiB",
                        total: "1.2 MiB",
                        rate: "8.4 MiB/s",
                    },
                    ProgressRow {
                        name: "very-long-capture-filename-that-elides.png",
                        pct: 41,
                        done: "0.5 MiB",
                        total: "1.2 MiB",
                        rate: "3.1 MiB/s",
                    },
                ],
            );
        });
    });
}

/// 시작 전 거부와 전송 중 실패의 확인·재시도 버튼을 비교한다.
pub fn draw_error(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "rejected before start (capacity)", |ui| {
            error_card(
                ui,
                theme,
                "sprint-42-demo.mp4",
                "capacity exceeded — transfers folder is at its 500 MiB limit",
                false,
            );
        });
        spec::cluster(ui, theme, "failed mid-transfer (retry)", |ui| {
            error_card(
                ui,
                theme,
                "sprint-42-demo.mp4",
                "connection lost while receiving",
                true,
            );
        });
    });
}

struct ProgressRow {
    name: &'static str,
    pct: u32,
    done: &'static str,
    total: &'static str,
    rate: &'static str,
}

/// 400px 프레임 셸 (bg-panel + 1px border-strong + modal shadow). item_spacing 0 —
/// 각 구역이 자체 패딩을 가진다.
fn frame(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .shadow(theme.shadow_modal().to_egui())
        .show(ui, |ui| {
            ui.set_width(FRAME_W.value());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.vertical(|ui| {
                ui.set_width(FRAME_W.value());
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                add(ui);
            });
        });
}

/// 헤더 띠 — glyph + 제목(+ 우측 trailing). 하단 separator.
fn header_band(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: icons::MockGlyph,
    glyph_color: egui::Color32,
    title: &str,
    trailing: Option<&str>,
) {
    let content_h = LogicalPx(20.0);
    let band_h = HEADER_PAD_Y.scaled(2.0) + content_h;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(FRAME_W.value(), band_h.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + PAD_X.value(),
            rect.top() + HEADER_PAD_Y.value(),
        ),
        egui::pos2(
            rect.right() - PAD_X.value(),
            rect.bottom() - HEADER_PAD_Y.value(),
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(&mut child, glyph, theme.icon_glyph_size_md, glyph_color);
    child.label(
        egui::RichText::new(title)
            .size(theme.font_size_max.value())
            .strong()
            .color(theme.text_primary().to_egui()),
    );
    if let Some(pct) = trailing {
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(pct)
                    .monospace()
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        });
    }
}

/// 진행 카드.
fn progress_card(ui: &mut egui::Ui, theme: &Theme, rows: &[ProgressRow]) {
    frame(ui, theme, |ui| {
        // 다중 파일이면 헤더 pct 는 전체 평균 대신 첫 행 기준(단일 파일이 표준).
        let head_pct = rows.first().map(|r| r.pct).unwrap_or(0);
        header_band(
            ui,
            theme,
            icons::DOWNLOAD,
            theme.text_muted().to_egui(),
            "Receiving file",
            Some(&format!("{head_pct}%")),
        );
        body_region(ui, |ui| {
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    ui.add_space(BODY_GAP.value());
                }
                progress_row(ui, theme, row);
            }
        });
    });
}

/// 한 파일 진행 행 — 파일명 → determinate bar → done/total · rate.
fn progress_row(ui: &mut egui::Ui, theme: &Theme, row: &ProgressRow) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        kit::icon(
            ui,
            icons::FILE,
            theme.icon_glyph_size_md,
            theme.text_muted().to_egui(),
        );
        let avail = ui.available_width();
        let name = elide_mono(ui, theme, row.name, avail);
        ui.label(
            egui::RichText::new(name)
                .monospace()
                .size(theme.font_size_body.value())
                .color(theme.text_primary().to_egui()),
        );
    });
    ui.add_space(BODY_GAP.value());
    progress_bar(ui, theme, row.pct);
    ui.add_space(BODY_GAP.value());
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} / {}", row.done, row.total))
                .monospace()
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(row.rate)
                    .monospace()
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        });
    });
}

/// determinate 4px progress bar — recessed track + accent fill (0ms 무애니).
/// 토큰: height=`--tasty-progress-height`(size-4 = spacing_xs) · radius=radius-sm ·
/// track=bg-app · fill=accent-primary.
fn progress_bar(ui: &mut egui::Ui, theme: &Theme, pct: u32) {
    let h = theme.spacing_xs.value(); // progress-height = size-4 = 4
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let r = theme.corner_radius_sm.value();
    ui.painter().rect_filled(rect, r, theme.bg_app().to_egui());
    let frac = (pct.min(100) as f32) / 100.0;
    if frac > 0.0 {
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac, h));
        ui.painter()
            .rect_filled(fill, r, theme.accent_primary().to_egui());
    }
}

/// 실패 카드.
fn error_card(ui: &mut egui::Ui, theme: &Theme, name: &str, reason: &str, retry: bool) {
    frame(ui, theme, |ui| {
        header_band(
            ui,
            theme,
            icons::ALERT_TRIANGLE,
            theme.accent_danger().to_egui(),
            "Transfer failed",
            None,
        );
        body_region(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(
                    egui::RichText::new(name)
                        .monospace()
                        .strong()
                        .size(theme.font_size_body.value())
                        .color(theme.text_primary().to_egui()),
                );
                ui.label(
                    egui::RichText::new(" could not be received.")
                        .size(theme.font_size_body.value())
                        .color(theme.text_secondary().to_egui()),
                );
            });
            ui.add_space(BODY_GAP.value());
            reason_well(ui, theme, reason);
        });
        // 푸터 버튼 — danger-fill 금지 (ghost/secondary 만).
        footer_buttons(ui, theme, |ui| {
            if retry {
                Button::new("Retry")
                    .variant(ButtonVariant::Secondary)
                    .size(ControlSize::Sm)
                    .show(ui, theme);
                Button::new("Dismiss")
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .show(ui, theme);
            } else {
                Button::new("Dismiss")
                    .variant(ButtonVariant::Secondary)
                    .size(ControlSize::Sm)
                    .show(ui, theme);
            }
        });
    });
}

/// command-well 패턴 — bg-app + 1px separator + radius, mono danger 텍스트.
fn reason_well(ui: &mut egui::Ui, theme: &Theme, reason: &str) {
    egui::Frame::new()
        .fill(theme.bg_app().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin {
            left: TRANSFER_CARD_PAD_X,
            right: TRANSFER_CARD_PAD_X,
            top: theme.spacing_sm.value() as i8,
            bottom: theme.spacing_sm.value() as i8,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(reason)
                    .monospace()
                    .size(theme.font_size_caption.value())
                    .color(theme.accent_danger().to_egui()),
            );
        });
}

/// 바디 region (padding 14, 전체폭).
fn body_region(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(BODY_PAD.value() as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui);
        });
}

/// 푸터 (padding 10/14, borderTop separator, 우측정렬). `add` 는 우→좌 순서로
/// 위젯을 넣는다(먼저 넣은 것이 우측 끝).
fn footer_buttons(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    let btn_h = LogicalPx(ControlSize::Sm.height(theme));
    let band_h = FOOTER_PAD_Y.scaled(2.0) + btn_h;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(FRAME_W.value(), band_h.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + PAD_X.value(),
            rect.top() + FOOTER_PAD_Y.value(),
        ),
        egui::pos2(
            rect.right() - PAD_X.value(),
            rect.bottom() - FOOTER_PAD_Y.value(),
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    add(&mut child);
}

/// mono 문자열을 폭에 맞게 앞은 두고 뒤를 `…` 로 자른다(폰트 메트릭 근사).
fn elide_mono(ui: &egui::Ui, theme: &Theme, s: &str, max_w: f32) -> String {
    let font = egui::FontId::monospace(theme.font_size_body.value());
    let w = |t: &str| {
        ui.fonts(|f| {
            f.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .rect
                .width()
        })
    };
    if w(s) <= max_w {
        return s.to_owned();
    }
    let mut cut = s.chars().collect::<Vec<_>>();
    while !cut.is_empty() {
        cut.pop();
        let candidate: String = cut.iter().collect::<String>() + "…";
        if w(&candidate) <= max_w {
            return candidate;
        }
    }
    "…".to_owned()
}
