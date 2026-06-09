//! Convert popup view 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/popup/convert.rs::draw_convert_view` 의 시각을 mock props
//! 로 재현. AppState/CoreState 비의존이라는 props 분리의 성과를 가시화하기 위함.
//!
//! 본체 의존: 없음. 본체 view 와 *시각/상수* 동기화 필요 — 본체 `ITEM_HEIGHT` 가
//! 바뀌면 여기도 같이 갱신해야 한다 (POC 단계 의도적 중복).

use tasty_type_appearance::theme::Theme;

/// 본체 src/adapters/ui/popup/convert.rs 의 상수와 동등.
const ITEM_HEIGHT: f32 = 24.0;

/// 본체 `ConvertItemView` 와 동등한 mock 구조.
#[derive(Clone)]
struct MockConvertItemView {
    label: &'static str,
    shortcut: Option<char>,
    is_current: bool,
}

/// 본체 `ConvertProps` 와 동등한 mock 구조.
struct MockConvertProps {
    items: Vec<MockConvertItemView>,
    selected_index: Option<usize>,
}

/// 본체 `draw_convert_view` 와 동등한 시각.
fn draw_mock_convert_view(ui: &mut egui::Ui, theme: &Theme, props: &MockConvertProps) {
    let popup_w = ui.available_width();

    for (idx, item) in props.items.iter().enumerate() {
        let is_current = item.is_current;
        let is_selected = props.selected_index == Some(idx);

        let shortcut_str: String = item.shortcut.map(|c| c.to_string()).unwrap_or_default();
        let label = if is_current {
            format!("  \u{2713} {}    {}", item.label, shortcut_str)
        } else {
            format!("    {}    {}", item.label, shortcut_str)
        };
        let text_color: egui::Color32 = if is_current {
            theme.overlay0
        } else {
            theme.text
        }
        .into();

        let sense = if is_current {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(popup_w, ITEM_HEIGHT), sense);

        let highlight = (!is_current && resp.hovered()) || is_selected;
        if highlight {
            ui.painter()
                .rect_filled(rect, 0.0, theme.hover_overlay.to_egui_premultiplied());
        }

        let text_pos = egui::pos2(
            rect.min.x + theme.spacing_sm.value(),
            rect.center().y - theme.font_size_body.value() / 2.0,
        );
        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_TOP,
            &label,
            egui::FontId::proportional(theme.font_size_body.value()),
            text_color,
        );
    }
}

/// "Popup frame" 처럼 보이도록 surface0 배경 + border 카드를 두르는 헬퍼.
fn with_popup_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    item_count: usize,
    paint: impl FnOnce(&mut egui::Ui),
) {
    const TITLE_BAR_HEIGHT: f32 = 28.0;
    const CONTENT_MARGIN: f32 = 4.0;
    const ITEM_SPACING: f32 = 4.0;

    let content_h =
        item_count as f32 * ITEM_HEIGHT + (item_count.saturating_sub(1)) as f32 * ITEM_SPACING;
    let total_h = TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + content_h + 1.0;
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

    // Title bar.
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
        "Convert surface",
        egui::FontId::proportional(theme.font_size_body.value()),
        text_color,
    );

    // Content 영역에 child Ui 생성.
    let content_top = title_rect.bottom() + CONTENT_MARGIN;
    let content_rect = egui::Rect::from_min_max(
        egui::pos2(frame_rect.min.x, content_top),
        egui::pos2(frame_rect.max.x, frame_rect.max.y - CONTENT_MARGIN),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    child.spacing_mut().item_spacing.y = ITEM_SPACING;
    paint(&mut child);
}

/// 4 가지 대표 상태:
/// 1. terminal 이 current — 다른 옵션 선택 가능 (정상)
/// 2. markdown 이 current — terminal 만 selectable, image 옵션 강조 없음
/// 3. selected_index 가 markdown 이 (키보드 선택 상태) — 강조 표시
/// 4. 단일 항목만 — edge case
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "ConvertProps + draw_convert_view — AppState/CoreState 비의존 view 함수.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    ui.label(
        egui::RichText::new("① Terminal current (정상 — 다른 kind 로 변환 가능):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockConvertProps {
        items: vec![
            MockConvertItemView {
                label: "Terminal",
                shortcut: Some('T'),
                is_current: true,
            },
            MockConvertItemView {
                label: "Markdown",
                shortcut: Some('M'),
                is_current: false,
            },
            MockConvertItemView {
                label: "Image",
                shortcut: Some('I'),
                is_current: false,
            },
            MockConvertItemView {
                label: "Explorer",
                shortcut: Some('E'),
                is_current: false,
            },
            MockConvertItemView {
                label: "HTML",
                shortcut: Some('H'),
                is_current: false,
            },
        ],
        selected_index: None,
    };
    with_popup_frame(ui, theme, 200.0, props.items.len(), |ui| {
        draw_mock_convert_view(ui, theme, &props);
    });

    ui.add_space(16.0);
    ui.label(
        egui::RichText::new("② Markdown current (terminal 만 강조 가능):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockConvertProps {
        items: vec![
            MockConvertItemView {
                label: "Terminal",
                shortcut: Some('T'),
                is_current: false,
            },
            MockConvertItemView {
                label: "Markdown",
                shortcut: Some('M'),
                is_current: true,
            },
            MockConvertItemView {
                label: "Image",
                shortcut: Some('I'),
                is_current: false,
            },
        ],
        selected_index: None,
    };
    with_popup_frame(ui, theme, 200.0, props.items.len(), |ui| {
        draw_mock_convert_view(ui, theme, &props);
    });

    ui.add_space(16.0);
    ui.label(
        egui::RichText::new("③ 키보드 선택 상태 (selected_index = Markdown 항목 강조):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockConvertProps {
        items: vec![
            MockConvertItemView {
                label: "Terminal",
                shortcut: Some('T'),
                is_current: true,
            },
            MockConvertItemView {
                label: "Markdown",
                shortcut: Some('M'),
                is_current: false,
            },
            MockConvertItemView {
                label: "Image",
                shortcut: Some('I'),
                is_current: false,
            },
        ],
        selected_index: Some(1),
    };
    with_popup_frame(ui, theme, 200.0, props.items.len(), |ui| {
        draw_mock_convert_view(ui, theme, &props);
    });

    ui.add_space(16.0);
    ui.label(
        egui::RichText::new("④ Edge — 단일 항목만 등록 (현재 kind 만 있음):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let props = MockConvertProps {
        items: vec![MockConvertItemView {
            label: "Terminal",
            shortcut: Some('T'),
            is_current: true,
        }],
        selected_index: None,
    };
    with_popup_frame(ui, theme, 200.0, props.items.len(), |ui| {
        draw_mock_convert_view(ui, theme, &props);
    });

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "⚠ 본체 view 함수와 시각 동기화. 키보드(Esc/Arrow/Enter/letter) 처리는 wrapper 책임 — 데모는 시각만.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
