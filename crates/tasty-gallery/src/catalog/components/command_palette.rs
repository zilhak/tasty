//! Command Palette popup view 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/popup/command_palette.rs::draw_command_palette_view`
//! 와 동일한 시각 layout 을 로컬 mock 으로 재현. AppState/CoreState 비의존
//! 이라는 props 분리 성과를 가시화한다.
//!
//! 본체 의존: 0. 본체 view 변경 시 시각 동기화는 수동 검증 (gallery 가
//! binary crate `tasty` 에 의존 불가하므로 struct/상수/아이콘 SVG 로컬 복제).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

const CONTENT_MARGIN: f32 = 4.0;
const POPUP_W: f32 = 520.0;

/// 본체 `icons::Icon` 의 로컬 mock. inline SVG 를 `egui_extras` svg 로더로
/// 텍스처화하고 `tint` 로 테마 색을 입힌다. SVG path 는 본체 `icons.rs` 에서 복제.
#[derive(Debug, Clone, Copy)]
struct MockIcon {
    svg: &'static str,
    uri: &'static str,
}

impl MockIcon {
    fn image(self, size: f32, tint: egui::Color32) -> egui::Image<'static> {
        egui::Image::from_bytes(self.uri, self.svg.as_bytes())
            .fit_to_exact_size(egui::vec2(size, size))
            .tint(tint)
    }
}

macro_rules! mock_icon {
    ($name:ident, $uri:literal, $body:literal) => {
        const $name: MockIcon = MockIcon {
            uri: concat!("bytes://gallery_cmd_icon_", $uri, ".svg"),
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
        };
    };
}

mock_icon!(
    SEARCH,
    "search",
    r#"<circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>"#
);
mock_icon!(PLUS, "plus", r#"<path d="M12 5v14M5 12h14"/>"#);
mock_icon!(
    TERM,
    "term",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#
);
mock_icon!(
    MD,
    "md",
    r#"<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 15V9l2.5 3L12 9v6M16 9v4m0 0 2-2m-2 2-2-2"/>"#
);
mock_icon!(
    SETTINGS,
    "settings",
    r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#
);
mock_icon!(
    SPLIT,
    "split",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>"#
);
mock_icon!(
    CLIPBOARD,
    "clipboard",
    r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/>"#
);

/// 본체 `CommandItemView` 와 동등한 로컬 mock.
#[derive(Debug, Clone)]
struct MockItem {
    label: String,
    shortcut: Option<String>,
    /// 행 좌측 leading 아이콘. 디자인 명시 명령에만 지정, 나머지는 빈 슬롯.
    icon: Option<MockIcon>,
}

/// 본체 `CommandPaletteProps` 와 동등.
struct MockProps<'a> {
    placeholder: &'a str,
    no_results_text: &'a str,
    items: Vec<MockItem>,
    selected_index: usize,
    query_buffer: &'a RefCell<String>,
    /// 푸터 힌트 — 네비게이션/실행/닫기 동작 라벨.
    hint_navigate: &'a str,
    hint_run: &'a str,
    hint_close: &'a str,
}

/// 본체 `draw_command_palette_view` 와 동등한 시각.
///
/// Gallery 는 action 을 무시 (단독 시각 검증 목적). 키보드 단축키 동작 mirroring
/// 은 생략 — gallery container 가 popup 이 아니라 일반 ui 영역이라 키 입력이
/// 다른 panel 로 가는 게 자연스러움.
fn draw_mock_view(ui: &mut egui::Ui, theme: &Theme, props: &MockProps<'_>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        // 검색 Input — leading 검색 아이콘 + TextEdit.
        ui.horizontal(|ui| {
            let icon_size = 16.0;
            let (icon_rect, _) =
                ui.allocate_exact_size(egui::vec2(icon_size, icon_size), egui::Sense::hover());
            SEARCH
                .image(icon_size, theme.text_muted().to_egui())
                .paint_at(ui, icon_rect);
            let mut buf = props.query_buffer.borrow_mut();
            ui.add(
                egui::TextEdit::singleline(&mut *buf)
                    .hint_text(props.placeholder)
                    .desired_width(ui.available_width())
                    .font(egui::TextStyle::Body),
            );
        });
        ui.separator();

        if props.items.is_empty() {
            ui.label(
                egui::RichText::new(props.no_results_text)
                    .color(theme.subtext0.to_egui())
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
                        if is_selected {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                theme.active_overlay.to_egui_premultiplied(),
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                theme.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        let color: egui::Color32 = if is_selected || resp.hovered() {
                            theme.text.into()
                        } else {
                            theme.subtext0.into()
                        };

                        // leading 아이콘 컬럼은 항상 폭을 예약해 라벨 정렬을 유지한다.
                        let pad_x = 8.0;
                        let icon_size = 16.0;
                        let icon_gap = 8.0;
                        if let Some(icon) = item.icon {
                            let icon_rect = egui::Rect::from_min_size(
                                egui::pos2(rect.min.x + pad_x, rect.center().y - icon_size / 2.0),
                                egui::vec2(icon_size, icon_size),
                            );
                            icon.image(icon_size, color).paint_at(ui, icon_rect);
                        }
                        let label_x = rect.min.x + pad_x + icon_size + icon_gap;
                        ui.painter().text(
                            egui::pos2(label_x, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &item.label,
                            egui::FontId::proportional(theme.font_size_body.value()),
                            color,
                        );

                        if let Some(shortcut) = &item.shortcut {
                            // Kbd 키캡 박스 — Theme 보더+배경 토큰으로 그린다.
                            let kbd_font =
                                egui::FontId::proportional(theme.font_size_caption.value());
                            let galley = ui.painter().layout_no_wrap(
                                shortcut.clone(),
                                kbd_font,
                                theme.text_muted().to_egui(),
                            );
                            let inner_pad = egui::vec2(6.0, 2.0);
                            let box_size = galley.size() + inner_pad * 2.0;
                            let box_rect = egui::Rect::from_min_size(
                                egui::pos2(
                                    rect.max.x - 8.0 - box_size.x,
                                    rect.center().y - box_size.y / 2.0,
                                ),
                                box_size,
                            );
                            ui.painter().rect_filled(
                                box_rect,
                                4.0,
                                theme.surface_hover().to_egui(),
                            );
                            ui.painter().rect_stroke(
                                box_rect,
                                4.0,
                                egui::Stroke::new(
                                    theme.border_width.value(),
                                    theme.border_strong().to_egui(),
                                ),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().galley(
                                box_rect.min + inner_pad,
                                galley,
                                theme.text_muted().to_egui(),
                            );
                        }
                    }
                });
        }

        // 푸터 — 키보드 힌트 행. mono 폰트 + muted 색. 기호는 고정키.
        ui.separator();
        let hint_color = theme.text_muted().to_egui();
        let hint_font = egui::FontId::monospace(theme.font_size_caption.value());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            for hint in [
                format!("↑↓ {}", props.hint_navigate),
                format!("↵ {}", props.hint_run),
                format!("esc {}", props.hint_close),
            ] {
                ui.label(
                    egui::RichText::new(hint)
                        .font(hint_font.clone())
                        .color(hint_color),
                );
            }
        });
    });
}

/// "Popup frame" 처럼 보이도록 surface0 배경 + border 카드를 두르는 헬퍼.
/// 본체 command_palette popup 은 headless (타이틀바 없음) 이므로 mock 도 동일.
fn with_popup_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    body_h: f32,
    paint: impl FnOnce(&mut egui::Ui),
) {
    let total_h = CONTENT_MARGIN * 2.0 + body_h;
    let (frame_rect, _) = ui.allocate_exact_size(egui::vec2(width, total_h), egui::Sense::hover());
    let painter = ui.painter_at(frame_rect);

    let bg: egui::Color32 = theme.surface0.into();
    let border: egui::Color32 = theme.surface2.into();

    painter.rect_filled(frame_rect, theme.corner_radius.value(), bg);
    painter.rect_stroke(
        frame_rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), border),
        egui::StrokeKind::Inside,
    );

    let content_rect = egui::Rect::from_min_max(
        egui::pos2(frame_rect.min.x + 8.0, frame_rect.min.y + CONTENT_MARGIN),
        egui::pos2(frame_rect.max.x - 8.0, frame_rect.max.y - CONTENT_MARGIN),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    paint(&mut child);
}

fn mk_item(label: &str, shortcut: Option<&str>, icon: Option<MockIcon>) -> MockItem {
    MockItem {
        label: label.to_string(),
        shortcut: shortcut.map(|s| s.to_string()),
        icon,
    }
}

/// 대표 상태 5 종:
/// 1. 빈 query — 전체 목록 (단축키 혼합 + leading 아이콘)
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

    // ① 빈 query — 전체 목록 (단축키 + leading 아이콘 혼합)
    ui.label(
        egui::RichText::new("① 빈 query — 전체 목록, 단축키 + leading 아이콘 혼합:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockProps {
        placeholder: "Type a command…",
        no_results_text: "No matching commands",
        items: vec![
            mk_item("New workspace", Some("Ctrl+Shift+N"), Some(PLUS)),
            mk_item("New tab", Some("Ctrl+T"), Some(TERM)),
            mk_item("Open markdown", None, Some(MD)),
            mk_item("Split pane vertical", Some("Ctrl+\\"), Some(SPLIT)),
            mk_item("Toggle sidebar", None, None),
            mk_item("Open settings", Some("Ctrl+,"), Some(SETTINGS)),
            mk_item("Clipboard viewer", None, Some(CLIPBOARD)),
            mk_item("Quit", Some("Ctrl+Q"), None),
        ],
        selected_index: 0,
        query_buffer: &buf1,
        hint_navigate: "navigate",
        hint_run: "run",
        hint_close: "close",
    };
    with_popup_frame(ui, theme, POPUP_W, 280.0, |ui| {
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
            mk_item("Close tab", Some("Ctrl+W"), None),
            mk_item("Close pane", Some("Ctrl+Shift+W"), None),
            mk_item("Close workspace", Some("Ctrl+Alt+W"), None),
            mk_item("Close all tabs in pane", None, None),
            mk_item("Close window", Some("Ctrl+Shift+Q"), None),
        ],
        selected_index: 2,
        query_buffer: &buf2,
        hint_navigate: "navigate",
        hint_run: "run",
        hint_close: "close",
    };
    with_popup_frame(ui, theme, POPUP_W, 230.0, |ui| {
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
        hint_navigate: "navigate",
        hint_run: "run",
        hint_close: "close",
    };
    with_popup_frame(ui, theme, POPUP_W, 110.0, |ui| {
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
                None,
            )
        })
        .collect();
    let props = MockProps {
        placeholder: "Type a command…",
        no_results_text: "No matching commands",
        items: many,
        selected_index: 7,
        query_buffer: &buf4,
        hint_navigate: "navigate",
        hint_run: "run",
        hint_close: "close",
    };
    with_popup_frame(ui, theme, POPUP_W, 340.0, |ui| {
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
            mk_item("Reload window", Some("Ctrl+R"), None),
            mk_item(
                "Toggle file handler picker preview overlay (debug only)",
                None,
                None,
            ),
            mk_item("Open recent workspace", Some("Ctrl+Shift+O"), None),
            mk_item(
                "Run extremely-long-named diagnostic command that should overflow the row width",
                None,
                None,
            ),
            mk_item("Focus next pane", Some("Alt+Tab"), None),
        ],
        selected_index: 1,
        query_buffer: &buf5,
        hint_navigate: "navigate",
        hint_run: "run",
        hint_close: "close",
    };
    with_popup_frame(ui, theme, POPUP_W, 230.0, |ui| {
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
