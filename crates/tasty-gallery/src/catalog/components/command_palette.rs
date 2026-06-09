//! Command Palette popup view 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/popup/command_palette.rs::draw_command_palette_view`
//! 와 동일한 시각 layout 을 로컬 mock 으로 재현. AppState/CoreState 비의존
//! 이라는 props 분리 성과를 가시화한다.
//!
//! 본체 의존: 0. 본체 view 변경 시 시각 동기화는 수동 검증 (gallery 가
//! binary crate `tasty` 에 의존 불가하므로 struct/상수 로컬 복제).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

const TITLE_BAR_HEIGHT: f32 = 28.0;
const CONTENT_MARGIN: f32 = 4.0;
const POPUP_W: f32 = 520.0;

/// 본체 `CommandItemView` 와 동등한 로컬 mock.
#[derive(Debug, Clone)]
struct MockItem {
    label: String,
    shortcut: Option<String>,
}

/// 본체 `CommandPaletteProps` 와 동등.
struct MockProps<'a> {
    placeholder: &'a str,
    no_results_text: &'a str,
    items: Vec<MockItem>,
    selected_index: usize,
    query_buffer: &'a RefCell<String>,
}

/// 본체 `draw_command_palette_view` 와 동등한 시각.
///
/// Gallery 는 action 을 무시 (단독 시각 검증 목적). 키보드 단축키 동작 mirroring
/// 은 생략 — gallery container 가 popup 이 아니라 일반 ui 영역이라 키 입력이
/// 다른 panel 로 가는 게 자연스러움.
fn draw_mock_view(ui: &mut egui::Ui, theme: &Theme, props: &MockProps<'_>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        let mut buf = props.query_buffer.borrow_mut();
        ui.add(
            egui::TextEdit::singleline(&mut *buf)
                .hint_text(props.placeholder)
                .desired_width(ui.available_width() - 8.0)
                .font(egui::TextStyle::Body),
        );
        drop(buf);
        ui.separator();

        if props.items.is_empty() {
            ui.label(
                egui::RichText::new(props.no_results_text)
                    .color(egui::Color32::from(theme.subtext0))
                    .italics(),
            );
        } else {
            let row_height = 24.0;
            let selected_idx = props.selected_index;
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    for (i, item) in props.items.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::click(),
                        );
                        let is_selected = i == selected_idx;
                        if is_selected || resp.hovered() {
                            // 본체는 theme.hover_overlay (premultiplied) 사용.
                            // gallery 에서는 surface1 으로 근사 — 시각만.
                            let bg: egui::Color32 = theme.surface1.into();
                            ui.painter().rect_filled(rect, 4.0, bg);
                        }
                        let color: egui::Color32 = if is_selected || resp.hovered() {
                            theme.text.into()
                        } else {
                            theme.subtext0.into()
                        };
                        ui.painter().text(
                            egui::pos2(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &item.label,
                            egui::FontId::proportional(theme.font_size_body.value()),
                            color,
                        );
                        if let Some(shortcut) = &item.shortcut {
                            ui.painter().text(
                                egui::pos2(rect.max.x - 8.0, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                shortcut,
                                egui::FontId::proportional(theme.font_size_body.value() - 1.0),
                                egui::Color32::from(theme.subtext0),
                            );
                        }
                    }
                });
        }
    });
}

/// "Popup frame" 처럼 보이도록 surface0 배경 + border 카드를 두르는 헬퍼.
fn with_popup_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    width: f32,
    body_h: f32,
    paint: impl FnOnce(&mut egui::Ui),
) {
    let total_h = TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + body_h;
    let (frame_rect, _) = ui.allocate_exact_size(egui::vec2(width, total_h), egui::Sense::hover());
    let painter = ui.painter_at(frame_rect);

    let bg: egui::Color32 = theme.surface0.into();
    let title_bg: egui::Color32 = theme.surface1.into();
    let border: egui::Color32 = theme.surface2.into();
    let text_color: egui::Color32 = theme.text.into();

    painter.rect_filled(frame_rect, theme.corner_radius.value(), bg);
    painter.rect_stroke(
        frame_rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), border),
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
        title_bg,
    );
    painter.text(
        egui::pos2(title_rect.min.x + 8.0, title_rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(theme.font_size_body.value()),
        text_color,
    );

    let content_top = title_rect.bottom() + CONTENT_MARGIN;
    let content_rect = egui::Rect::from_min_max(
        egui::pos2(frame_rect.min.x + 8.0, content_top + 4.0),
        egui::pos2(frame_rect.max.x - 8.0, frame_rect.max.y - CONTENT_MARGIN),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    paint(&mut child);
}

fn mk_item(label: &str, shortcut: Option<&str>) -> MockItem {
    MockItem {
        label: label.to_string(),
        shortcut: shortcut.map(|s| s.to_string()),
    }
}

/// 대표 상태 5 종:
/// 1. 빈 query — 전체 목록 (단축키 혼합)
/// 2. 짧은 query — 필터된 5건
/// 3. 결과 0건
/// 4. 100건 (스크롤 검증)
/// 5. 단축키 있는 / 없는 혼합 (긴 라벨 wrap)
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "CommandPaletteProps + draw_command_palette_view — AppState/CoreState 비의존 view 함수.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    let buf1 = RefCell::new(String::new());
    let buf2 = RefCell::new(String::from("clos"));
    let buf3 = RefCell::new(String::from("xyzzy"));
    let buf4 = RefCell::new(String::new());
    let buf5 = RefCell::new(String::new());

    // ① 빈 query — 전체 목록 (단축키 혼합)
    ui.label(
        egui::RichText::new("① 빈 query — 전체 목록, 단축키 혼합:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockProps {
        placeholder: "Type a command…",
        no_results_text: "No matching commands",
        items: vec![
            mk_item("New workspace", Some("Ctrl+Shift+N")),
            mk_item("Close tab", Some("Ctrl+W")),
            mk_item("Split pane right", Some("Ctrl+\\")),
            mk_item("Toggle sidebar", None),
            mk_item("Open settings", Some("Ctrl+,")),
            mk_item("Quit", Some("Ctrl+Q")),
        ],
        selected_index: 0,
        query_buffer: &buf1,
    };
    with_popup_frame(ui, theme, "Command Palette", POPUP_W, 220.0, |ui| {
        draw_mock_view(ui, theme, &props);
    });
    ui.add_space(16.0);

    // ② 짧은 query — 필터된 5건 (선택 인덱스 2)
    ui.label(
        egui::RichText::new("② 짧은 query (\"clos\") — 필터된 5건, 선택 인덱스 2:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockProps {
        placeholder: "Type a command…",
        no_results_text: "No matching commands",
        items: vec![
            mk_item("Close tab", Some("Ctrl+W")),
            mk_item("Close pane", Some("Ctrl+Shift+W")),
            mk_item("Close workspace", Some("Ctrl+Alt+W")),
            mk_item("Close all tabs in pane", None),
            mk_item("Close window", Some("Ctrl+Shift+Q")),
        ],
        selected_index: 2,
        query_buffer: &buf2,
    };
    with_popup_frame(ui, theme, "Command Palette", POPUP_W, 200.0, |ui| {
        draw_mock_view(ui, theme, &props);
    });
    ui.add_space(16.0);

    // ③ 결과 0건
    ui.label(
        egui::RichText::new("③ 결과 0건 — placeholder 텍스트:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockProps {
        placeholder: "Type a command…",
        no_results_text: "No matching commands",
        items: vec![],
        selected_index: 0,
        query_buffer: &buf3,
    };
    with_popup_frame(ui, theme, "Command Palette", POPUP_W, 90.0, |ui| {
        draw_mock_view(ui, theme, &props);
    });
    ui.add_space(16.0);

    // ④ 100건 (스크롤)
    ui.label(
        egui::RichText::new("④ 100건 — ScrollArea 가상화:").color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let many: Vec<MockItem> = (0..100)
        .map(|i| {
            let shortcut = if i % 5 == 0 {
                Some(format!("Ctrl+Alt+{}", i % 10))
            } else {
                None
            };
            mk_item(
                &format!("Action #{i:03} — generated for scroll test"),
                shortcut.as_deref(),
            )
        })
        .collect();
    let props = MockProps {
        placeholder: "Type a command…",
        no_results_text: "No matching commands",
        items: many,
        selected_index: 7,
        query_buffer: &buf4,
    };
    with_popup_frame(ui, theme, "Command Palette", POPUP_W, 320.0, |ui| {
        draw_mock_view(ui, theme, &props);
    });
    ui.add_space(16.0);

    // ⑤ 단축키 mix + 긴 라벨
    ui.label(
        egui::RichText::new("⑤ 단축키 있음/없음 mix + 긴 라벨 overflow:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockProps {
        placeholder: "Type a command…",
        no_results_text: "No matching commands",
        items: vec![
            mk_item("Reload window", Some("Ctrl+R")),
            mk_item(
                "Toggle file handler picker preview overlay (debug only)",
                None,
            ),
            mk_item("Open recent workspace", Some("Ctrl+Shift+O")),
            mk_item(
                "Run extremely-long-named diagnostic command that should overflow the row width",
                None,
            ),
            mk_item("Focus next pane", Some("Alt+Tab")),
        ],
        selected_index: 1,
        query_buffer: &buf5,
    };
    with_popup_frame(ui, theme, "Command Palette", POPUP_W, 200.0, |ui| {
        draw_mock_view(ui, theme, &props);
    });

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "⚠ 본체 view 와 시각 동기화. 키 입력 (Escape/↑/↓/Enter) 은 view 내부 — 갤러리는 시각만.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
