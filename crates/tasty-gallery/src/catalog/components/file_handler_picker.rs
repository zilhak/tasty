//! File Handler Picker popup 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/popup/file_handler_picker.rs::draw_file_handler_picker_view`
//! 가 표현하는 시각 상태를 mock props 로 재현. 본체와 *시각 동일* 하지만
//! gallery 가 본체 binary 에 의존할 수 없으므로 view 로직은 로컬 미러
//! (POC 패턴 — `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).
//!
//! 대표 상태:
//! 1. 빈 상태 (candidates 0 + recent 0)
//! 2. 단일 후보 + recent 비어 있음
//! 3. 다중 후보 + recent 3 개 (정상 케이스)
//! 4. 매우 긴 파일 경로 + detector 미탐지 (unknown)
//! 5. 다수 후보 (스크롤 가능 영역 검증)

use tasty_type_appearance::theme::Theme;

use crate::catalog::popup_frame::{self, CONTENT_MARGIN, ContentInset, TITLE_BAR_HEIGHT};

const ITEM_HEIGHT: f32 = 22.0;
const LIST_MIN_HEIGHT: f32 = 4.0 * ITEM_HEIGHT;
const LIST_MAX_HEIGHT: f32 = 10.0 * ITEM_HEIGHT;
const VERTICAL_PADDING: f32 = 8.0;
const HORIZONTAL_MARGIN: f32 = 8.0;
const POPUP_WIDTH: f32 = 480.0;

#[derive(Clone, Debug)]
struct EntryView {
    id: String,
    display: String,
}

struct PickerProps<'a> {
    theme: &'a Theme,
    target_display: &'a str,
    detector_label: Option<&'a str>,
    candidates: &'a [EntryView],
    recent: &'a [EntryView],
    selected_id: Option<&'a str>,

    target_label: &'a str,
    format_label: &'a str,
    unknown_format_label: &'a str,
    candidates_heading: &'a str,
    recent_heading: &'a str,
    empty_label: &'a str,
    open_button_label: &'a str,
    cancel_button_label: &'a str,
}

/// 본체 `draw_file_handler_picker_view` 의 시각 미러 (gallery 측 복제).
/// Action 반환은 카탈로그 시각 검증 목적이라 생략 — 클릭/Esc 처리는 본체 wrapper 책임.
fn draw_file_handler_picker_view(ui: &mut egui::Ui, props: &PickerProps<'_>) {
    let th = props.theme;

    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(HORIZONTAL_MARGIN, 0.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    // Header
    ui.label(
        egui::RichText::new(format!("{} {}", props.target_label, props.target_display))
            .size(th.font_size_body.value())
            .color(egui::Color32::from(th.text)),
    );
    let detector_text = props.detector_label.unwrap_or(props.unknown_format_label);
    ui.label(
        egui::RichText::new(format!("{} {}", props.format_label, detector_text))
            .size(th.font_size_caption.value())
            .color(egui::Color32::from(th.subtext0)),
    );

    ui.add_space(VERTICAL_PADDING);

    let is_empty = props.candidates.is_empty() && props.recent.is_empty();

    if is_empty {
        ui.label(
            egui::RichText::new(props.empty_label)
                .size(th.font_size_body.value())
                .color(egui::Color32::from(th.subtext1)),
        );
        ui.add_space(VERTICAL_PADDING);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.button(props.cancel_button_label); // gallery 데모 — 클릭 처리 없음
        });
        return;
    }

    let list_height = {
        let rows = props.candidates.len().max(props.recent.len());
        (rows.max(4) as f32 * ITEM_HEIGHT).clamp(LIST_MIN_HEIGHT, LIST_MAX_HEIGHT)
    };

    let total_w = ui.available_width();
    let col_w = (total_w - 8.0) / 2.0;

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(col_w);
            ui.set_max_width(col_w);
            ui.label(
                egui::RichText::new(props.candidates_heading)
                    .size(th.font_size_caption.value())
                    .color(egui::Color32::from(th.overlay1)),
            );
            egui::ScrollArea::vertical()
                .id_salt("gallery_fhp_candidates")
                .max_height(list_height)
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    draw_list(ui, th, props.candidates, props.selected_id, col_w);
                });
        });

        ui.add_space(8.0);

        ui.vertical(|ui| {
            ui.set_min_width(col_w);
            ui.set_max_width(col_w);
            ui.label(
                egui::RichText::new(props.recent_heading)
                    .size(th.font_size_caption.value())
                    .color(egui::Color32::from(th.overlay1)),
            );
            egui::ScrollArea::vertical()
                .id_salt("gallery_fhp_recent")
                .max_height(list_height)
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    draw_list(ui, th, props.recent, props.selected_id, col_w);
                });
        });
    });

    ui.add_space(VERTICAL_PADDING);

    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let enabled = props.selected_id.is_some();
            ui.add_enabled_ui(enabled, |ui| {
                let _ = ui.button(props.open_button_label); // gallery 데모 — 클릭 처리 없음
            });
            let _ = ui.button(props.cancel_button_label); // gallery 데모 — 클릭 처리 없음
        });
    });
}

fn draw_list(
    ui: &mut egui::Ui,
    th: &Theme,
    items: &[EntryView],
    selected_id: Option<&str>,
    col_w: f32,
) {
    for entry in items {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(col_w, ITEM_HEIGHT), egui::Sense::click());

        let is_selected = selected_id == Some(entry.id.as_str());

        if is_selected {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
        }
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
        }

        ui.painter().text(
            egui::pos2(
                rect.min.x + 4.0,
                rect.center().y - th.font_size_caption.value() / 2.0,
            ),
            egui::Align2::LEFT_TOP,
            &entry.display,
            egui::FontId::proportional(th.font_size_caption.value()),
            if is_selected {
                egui::Color32::from(th.text)
            } else {
                egui::Color32::from(th.subtext0)
            },
        );
    }
}

fn popup_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    body_h: f32,
    paint: impl FnOnce(&mut egui::Ui),
) {
    let total_h = TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + body_h;
    popup_frame::draw(
        ui,
        theme,
        title,
        POPUP_WIDTH,
        total_h,
        ContentInset::FLUSH,
        paint,
    );
}

fn estimate_body_height(cand_n: usize, recent_n: usize, is_empty: bool) -> f32 {
    const HEADER_H: f32 = 36.0;
    const BUTTON_H: f32 = 28.0;

    let body_h = if is_empty {
        HEADER_H + VERTICAL_PADDING + ITEM_HEIGHT + VERTICAL_PADDING + BUTTON_H
    } else {
        let rows = cand_n.max(recent_n);
        let list_h = (rows.max(4) as f32 * ITEM_HEIGHT).clamp(LIST_MIN_HEIGHT, LIST_MAX_HEIGHT);
        // +14: column heading 라벨 1 행분.
        HEADER_H + VERTICAL_PADDING + list_h + 14.0 + VERTICAL_PADDING + BUTTON_H
    };
    body_h + 8.0
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "FileHandlerPickerProps + draw_file_handler_picker_view — AppState/CoreState 비의존.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Wrapper: src/adapters/ui/popup/file_handler_picker.rs::draw_file_handler_picker",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    let target_label = "Target:";
    let format_label = "Format:";
    let unknown_format_label = "unknown";
    let candidates_heading = "Candidates";
    let recent_heading = "Recent";
    let empty_label = "No handlers registered for this file type.";
    let open_button_label = "Open";
    let cancel_button_label = "Cancel";

    egui::ScrollArea::vertical()
        .id_salt("file_handler_picker_demo_scroll")
        .show(ui, |ui| {
            // Case 1 — Empty
            ui.label(
                egui::RichText::new("Case 1 — Empty (candidates 0 + recent 0)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            popup_frame(
                ui,
                theme,
                "Pick handler: /tmp/example.unknownext",
                estimate_body_height(0, 0, true),
                |ui| {
                    let props = PickerProps {
                        theme,
                        target_display: "/tmp/example.unknownext",
                        detector_label: None,
                        candidates: &[],
                        recent: &[],
                        selected_id: None,
                        target_label,
                        format_label,
                        unknown_format_label,
                        candidates_heading,
                        recent_heading,
                        empty_label,
                        open_button_label,
                        cancel_button_label,
                    };
                    draw_file_handler_picker_view(ui, &props);
                },
            );
            ui.add_space(16.0);

            // Case 2 — single candidate, no recent
            ui.label(
                egui::RichText::new("Case 2 — Single candidate, recent 비어 있음")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let cands_single = vec![EntryView {
                id: "host/markdown-viewer".into(),
                display: "host/markdown-viewer".into(),
            }];
            popup_frame(
                ui,
                theme,
                "Pick handler: README.md",
                estimate_body_height(1, 0, false),
                |ui| {
                    let props = PickerProps {
                        theme,
                        target_display: "README.md",
                        detector_label: Some("markdown"),
                        candidates: &cands_single,
                        recent: &[],
                        selected_id: Some("host/markdown-viewer"),
                        target_label,
                        format_label,
                        unknown_format_label,
                        candidates_heading,
                        recent_heading,
                        empty_label,
                        open_button_label,
                        cancel_button_label,
                    };
                    draw_file_handler_picker_view(ui, &props);
                },
            );
            ui.add_space(16.0);

            // Case 3 — 정상: 다중 후보 + recent 3 개
            ui.label(
                egui::RichText::new("Case 3 — 정상 (candidates 3 + recent 3, selected=후보 1)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let cands_3 = vec![
                EntryView {
                    id: "host/markdown-viewer".into(),
                    display: "host/markdown-viewer".into(),
                },
                EntryView {
                    id: "user/my-md-handler".into(),
                    display: "user/my-md-handler".into(),
                },
                EntryView {
                    id: "com.tasty.docs/render".into(),
                    display: "com.tasty.docs/render".into(),
                },
            ];
            let recent_3 = vec![
                EntryView {
                    id: "user/my-pdf-opener".into(),
                    display: "user/my-pdf-opener".into(),
                },
                EntryView {
                    id: "host/image-viewer".into(),
                    display: "host/image-viewer".into(),
                },
                EntryView {
                    id: "com.tasty.docs/render".into(),
                    display: "com.tasty.docs/render".into(),
                },
            ];
            popup_frame(
                ui,
                theme,
                "Pick handler: docs/architecture.md",
                estimate_body_height(3, 3, false),
                |ui| {
                    let props = PickerProps {
                        theme,
                        target_display: "docs/architecture.md",
                        detector_label: Some("markdown"),
                        candidates: &cands_3,
                        recent: &recent_3,
                        selected_id: Some("host/markdown-viewer"),
                        target_label,
                        format_label,
                        unknown_format_label,
                        candidates_heading,
                        recent_heading,
                        empty_label,
                        open_button_label,
                        cancel_button_label,
                    };
                    draw_file_handler_picker_view(ui, &props);
                },
            );
            ui.add_space(16.0);

            // Case 4 — 매우 긴 경로 + detector 미탐지
            ui.label(
                egui::RichText::new(
                    "Case 4 — 매우 긴 경로 (shorten_target 적용 가정) + detector 미탐지",
                )
                .strong()
                .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let cands_short = vec![EntryView {
                id: "user/binary-inspector".into(),
                display: "user/binary-inspector".into(),
            }];
            popup_frame(
                ui,
                theme,
                "Pick handler: .../path/to/file.dat",
                estimate_body_height(1, 0, false),
                |ui| {
                    let props = PickerProps {
                        theme,
                        target_display: ".../very/long/nested/path/file.dat",
                        detector_label: None,
                        candidates: &cands_short,
                        recent: &[],
                        selected_id: None,
                        target_label,
                        format_label,
                        unknown_format_label,
                        candidates_heading,
                        recent_heading,
                        empty_label,
                        open_button_label,
                        cancel_button_label,
                    };
                    draw_file_handler_picker_view(ui, &props);
                },
            );
            ui.add_space(16.0);

            // Case 5 — 다수 후보 (스크롤 영역)
            ui.label(
                egui::RichText::new("Case 5 — Many (candidates 12, list 최대 높이 + 스크롤)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            let cands_many: Vec<EntryView> = (0..12)
                .map(|i| EntryView {
                    id: format!("plugin.example/handler-{i:02}"),
                    display: format!("plugin.example/handler-{i:02}"),
                })
                .collect();
            popup_frame(
                ui,
                theme,
                "Pick handler: image.png",
                estimate_body_height(12, 0, false),
                |ui| {
                    let props = PickerProps {
                        theme,
                        target_display: "image.png",
                        detector_label: Some("image/png"),
                        candidates: &cands_many,
                        recent: &[],
                        selected_id: Some("plugin.example/handler-03"),
                        target_label,
                        format_label,
                        unknown_format_label,
                        candidates_heading,
                        recent_heading,
                        empty_label,
                        open_button_label,
                        cancel_button_label,
                    };
                    draw_file_handler_picker_view(ui, &props);
                },
            );

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(
                    "Note: 클릭/더블클릭/Esc/[열기]/[취소] 동작은 본체 view 가 Action 으로 \
                     반환해 wrapper 가 state mutation. 갤러리는 시각만 확인.",
                )
                .small()
                .color(egui::Color32::from(theme.subtext0)),
            );
        });
}
