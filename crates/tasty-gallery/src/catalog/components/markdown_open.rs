//! Markdown Open popup 데모 (Tier 3 — props/view 분리 결과 시각 검증).
//!
//! 본체 `src/adapters/ui/popup/file_open.rs::draw_markdown_open_view` 의 *시각* 만
//! mock props 로 재현. 본체 의존성 (AppState, CoreState, i18n) 은 끌어오지 않고
//! 로컬로 복제 (POC 분리 패턴 §5). 추후 view 가 공유 lib crate 로 이동하면 mirror 제거.
//!
//! 데모 상태:
//! - 빈 recents (placeholder)
//! - 3건 recents (정상)
//! - 10건 recents (스크롤 최대)
//! - 매우 긴 경로 + 짧은 경로 혼합
//! - 에러 상태 (file not found)

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

const ITEM_HEIGHT: f32 = 22.0;
const MAX_RECENT: usize = 10;
const HORIZONTAL_MARGIN: f32 = 8.0;
const ITEM_SPACING_Y: f32 = 4.0;
const TITLE_BAR_HEIGHT: f32 = 28.0;
const CONTENT_MARGIN: f32 = 4.0;

/// 본체 file_open.rs::MarkdownOpenProps mirror — gallery 전용 로컬 복제.
struct MarkdownOpenProps<'a> {
    theme: &'a Theme,
    path_input: &'a mut String,
    recents: &'a [String],
    error: Option<&'a str>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkdownOpenAction {
    None,
    Close,
    Cancel,
    Confirm,
    BrowseFile,
}

fn markdown_open_popup_size(recent_count: usize) -> egui::Vec2 {
    let base = 16.0 + ITEM_SPACING_Y + 4.0 + 22.0 + ITEM_SPACING_Y + 8.0 + 24.0;
    let recent_h = if recent_count > 0 {
        16.0 + ITEM_SPACING_Y
            + 2.0
            + (recent_count.min(MAX_RECENT) as f32 * ITEM_HEIGHT)
            + ITEM_SPACING_Y
            + 4.0
    } else {
        0.0
    };
    egui::vec2(
        360.0,
        TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + base + recent_h,
    )
}

/// 본체 draw_markdown_open_view 의 시각 mirror.
/// 본체와 *동일한 layout 식* 을 사용하여 갤러리에서 회귀 확인 가능.
fn draw_markdown_open_view(
    ui: &mut egui::Ui,
    props: &mut MarkdownOpenProps<'_>,
) -> MarkdownOpenAction {
    let th = props.theme;

    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(HORIZONTAL_MARGIN, 0.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    ui.label(
        egui::RichText::new("Path")
            .size(th.font_size_body.value())
            .color(th.subtext1),
    );
    ui.add_space(4.0);

    let mut action = MarkdownOpenAction::None;

    ui.horizontal(|ui| {
        let _resp = ui.add_sized(
            [ui.available_width() - 30.0, 22.0],
            egui::TextEdit::singleline(props.path_input)
                .font(egui::FontId::proportional(th.font_size_body.value()))
                .margin(egui::Margin::symmetric(4, 2)),
        );

        if ui
            .add_sized([26.0, 22.0], egui::Button::new("\u{1F4C2}"))
            .clicked()
        {
            action = MarkdownOpenAction::BrowseFile;
        }
    });

    if let Some(err) = props.error {
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(err)
                .size(th.font_size_caption.value())
                .color(egui::Color32::from(th.accent_danger())),
        );
    }

    ui.add_space(8.0);

    if !props.recents.is_empty() {
        ui.label(
            egui::RichText::new("Recent files")
                .size(th.font_size_caption.value())
                .color(egui::Color32::from(th.overlay1)),
        );
        ui.add_space(2.0);

        egui::ScrollArea::vertical()
            .id_salt(ui.id().with("recents_scroll"))
            .max_height(MAX_RECENT as f32 * ITEM_HEIGHT)
            .drag_to_scroll(false)
            .show(ui, |ui| {
                for entry in props.recents.iter().take(MAX_RECENT) {
                    let display = shorten_path(entry);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), ITEM_HEIGHT),
                        egui::Sense::click(),
                    );
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            th.hover_overlay.to_egui_premultiplied(),
                        );
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    ui.painter().text(
                        egui::pos2(
                            rect.min.x + 4.0,
                            rect.center().y - th.font_size_caption.value() / 2.0,
                        ),
                        egui::Align2::LEFT_TOP,
                        &display,
                        egui::FontId::proportional(th.font_size_caption.value()),
                        th.subtext0.into(),
                    );
                    if resp.clicked() {
                        *props.path_input = entry.clone();
                        action = MarkdownOpenAction::Confirm;
                    }
                }
            });
    }

    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Cancel").clicked() {
                action = MarkdownOpenAction::Cancel;
            }
            if ui.button("OK").clicked() {
                action = MarkdownOpenAction::Confirm;
            }
        });
    });

    action
}

fn shorten_path(path: &str) -> String {
    if path.len() <= 50 {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        return path.to_string();
    }
    format!(".../{}", parts[parts.len() - 2..].join("/"))
}

struct CaseDef {
    title: &'static str,
    recents: Vec<String>,
    error: Option<&'static str>,
    initial_path: &'static str,
}

fn cases() -> Vec<CaseDef> {
    vec![
        CaseDef {
            title: "Empty — placeholder",
            recents: vec![],
            error: None,
            initial_path: "",
        },
        CaseDef {
            title: "3 recents — normal",
            recents: vec![
                "/Users/dev/notes/today.md".to_string(),
                "/Users/dev/notes/meeting.md".to_string(),
                "/Users/dev/notes/spec.md".to_string(),
            ],
            error: None,
            initial_path: "",
        },
        CaseDef {
            title: "10 recents — scroll cap",
            recents: (1..=10)
                .map(|i| format!("/Users/dev/notes/entry-{i:02}.md"))
                .collect(),
            error: None,
            initial_path: "",
        },
        CaseDef {
            title: "Long-path mix",
            recents: vec![
                "/Users/dev/very/deeply/nested/path/that/should/be/shortened/with/ellipsis/notes/long-name.md".to_string(),
                "/short.md".to_string(),
                "/Users/dev/docs/another/regular/length/path.md".to_string(),
            ],
            error: None,
            initial_path: "",
        },
        CaseDef {
            title: "Error — file not found",
            recents: vec!["/Users/dev/notes/exists.md".to_string()],
            error: Some("File not found"),
            initial_path: "/tmp/nope.md",
        },
    ]
}

thread_local! {
    static MOCK_BUFFERS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn ensure_buffers(count: usize, initials: &[&str]) {
    MOCK_BUFFERS.with(|b| {
        let mut bufs = b.borrow_mut();
        if bufs.len() != count {
            bufs.clear();
            for s in initials.iter().take(count) {
                bufs.push((*s).to_string());
            }
        }
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "draw_markdown_open_view(ui, &mut MarkdownOpenProps { theme, path_input, recents, error })",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    let case_defs = cases();
    let initials: Vec<&str> = case_defs.iter().map(|c| c.initial_path).collect();
    ensure_buffers(case_defs.len(), &initials);

    egui::ScrollArea::vertical()
        .id_salt("markdown_open_demo_scroll")
        .show(ui, |ui| {
            for (i, def) in case_defs.iter().enumerate() {
                ui.label(
                    egui::RichText::new(format!("Case #{i} — {}", def.title))
                        .size(theme.font_size_body.value())
                        .color(egui::Color32::from(theme.text)),
                );
                ui.add_space(2.0);

                let size = markdown_open_popup_size(def.recents.len());
                ui.label(
                    egui::RichText::new(format!(
                        "popup_size = {:.0} × {:.0}",
                        size.x, size.y
                    ))
                    .small()
                    .color(egui::Color32::from(theme.subtext0)),
                );
                ui.add_space(4.0);

                let (frame_rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter_at(frame_rect);
                painter.rect_filled(
                    frame_rect,
                    theme.corner_radius.value(),
                    egui::Color32::from(theme.surface0),
                );
                painter.rect_stroke(
                    frame_rect,
                    theme.corner_radius.value(),
                    egui::Stroke::new(
                        theme.border_width.value(),
                        egui::Color32::from(theme.surface2),
                    ),
                    egui::StrokeKind::Inside,
                );
                let title_rect = egui::Rect::from_min_size(
                    frame_rect.min,
                    egui::vec2(frame_rect.width(), TITLE_BAR_HEIGHT),
                );
                painter.rect_filled(
                    title_rect,
                    egui::CornerRadius {
                        nw: theme.corner_radius.value() as u8,
                        ne: theme.corner_radius.value() as u8,
                        sw: 0,
                        se: 0,
                    },
                    egui::Color32::from(theme.surface1),
                );
                painter.text(
                    egui::pos2(title_rect.min.x + 8.0, title_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Open Markdown",
                    egui::FontId::proportional(theme.font_size_body.value()),
                    egui::Color32::from(theme.text),
                );

                let content_top = title_rect.bottom() + CONTENT_MARGIN;
                let content_rect = egui::Rect::from_min_max(
                    egui::pos2(frame_rect.min.x, content_top),
                    egui::pos2(frame_rect.max.x, frame_rect.max.y - CONTENT_MARGIN),
                );
                let mut child_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .id_salt(format!("markdown_open_case_{i}")),
                );

                MOCK_BUFFERS.with(|b| {
                    let mut bufs = b.borrow_mut();
                    let mut props = MarkdownOpenProps {
                        theme,
                        path_input: &mut bufs[i],
                        recents: &def.recents,
                        error: def.error,
                    };
                    let _action = draw_markdown_open_view(&mut child_ui, &mut props);
                    // 카탈로그는 시각 검증 — action 은 표시만.
                });

                ui.add_space(16.0);
            }

            ui.label(
                egui::RichText::new(
                    "⚠ Gallery mirror — 실제 file-picker / IPC / AppState mutation 은 본체 wrapper.",
                )
                .small()
                .color(egui::Color32::from(theme.subtext0)),
            );
        });
}
