//! Search bar overlay 데모 (Overlays).
//!
//! 본체 `src/adapters/ui/search_bar.rs::draw_search_bar` 가 표현하는 시각을
//! 로컬 mock 으로 재현. 디자인 canonical: `overlays/search_bar.jsx` (360×28).
//!
//! 본체 의존: 0. gallery 가 binary crate `tasty` 에 의존 불가하므로 view 의
//! 시각 layout 만 복제하고 상태(쿼리/매치/옵션)는 로컬 mock props 로 주입한다.
//! 본체 view 변경 시 시각 동기화는 수동 검증.
//!
//! 한 행 (4px gap): input(flex, min 60) · 카운터(40, center) · ▲▼ nav ·
//! Aa/.*/ab 토글 · │ divider · ✕ close. nav 는 매치 0 이면 disabled,
//! 토글은 active 면 active_overlay 배경.

use tasty_type_appearance::theme::Theme;

/// 디자인 canonical 폭 (search_bar.jsx: `width: 360`).
const BAR_W: f32 = 360.0;

/// 본체 `icons::Icon` 의 로컬 mock. inline SVG 를 `egui_extras` svg 로더로
/// 텍스처화하고 `tint` 로 테마 색을 입힌다. SVG path 는 본체 `icons.rs` 복제.
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
            uri: concat!("bytes://gallery_search_icon_", $uri, ".svg"),
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
        };
    };
}

// 본체 icons::CHEVRON_UP / CHEVRON_DOWN / CLOSE 와 동일한 path.
mock_icon!(CHEVRON_UP, "chevron_up", r#"<path d="m18 15-6-6-6 6"/>"#);
mock_icon!(CHEVRON_DOWN, "chevron_down", r#"<path d="m6 9 6 6 6-6"/>"#);
mock_icon!(CLOSE, "close", r#"<path d="M18 6 6 18M6 6l12 12"/>"#);

/// 본체 search 상태 중 시각에 영향을 주는 부분만 추린 로컬 mock props.
struct MockProps<'a> {
    /// 입력창 내용. 빈 문자열이면 placeholder 표시.
    query: &'a str,
    /// 총 매치 수. 0 이면 nav disabled + 카운터 `0/0`.
    match_count: usize,
    /// 현재 매치 인덱스 (0-base). 카운터는 `current+1`/total 로 표시.
    current_index: usize,
    /// 정규식 에러 등으로 쿼리가 있는데 결과가 0 인 상태 → 카운터 danger.
    error: bool,
    case_active: bool,  // "Aa" — 대소문자 구분 켜짐
    regex_active: bool, // ".*"
    word_active: bool,  // "ab" — whole word
}

/// 본체 `draw_search_bar` 와 동등한 시각. action 은 무시 (단독 시각 검증).
fn draw_mock_bar(ui: &mut egui::Ui, theme: &Theme, props: &MockProps<'_>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // 입력창 폭: 가용폭 - (카운터 40 + nav 2 + 토글 3 = 버튼 5개 + 그 사이 간격 6).
        let spacing = ui.spacing().item_spacing.x;
        let btn_size = theme.item_height_tab.value();
        let reserved = 40.0 + 5.0 * btn_size + 6.0 * spacing;
        let input_width = (ui.available_width() - reserved).max(60.0);

        // 본체는 egui::TextEdit. gallery 는 상태 주입 mock 이므로 read-only
        // 버퍼로 동일 시각만 재현 (placeholder 포함).
        let mut buf = props.query.to_string();
        ui.add_enabled(
            false,
            egui::TextEdit::singleline(&mut buf)
                .hint_text(
                    egui::RichText::new("Search...").color(theme.text_muted().to_egui()),
                )
                .desired_width(input_width)
                .font(egui::TextStyle::Body),
        );

        // Status counter — 고정폭 40, center. 쿼리가 있는데 결과 0(정규식 에러
        // 포함)이면 danger, 그 외(빈 쿼리 / 정상 매치)는 muted.
        // (본체 search_bar.rs:103-117 미러)
        let has_query = !props.query.is_empty();
        let no_results = props.match_count == 0 || props.error;
        let (counter_text, counter_color) = if no_results {
            let color = if has_query {
                theme.accent_danger()
            } else {
                theme.text_muted()
            };
            ("0/0".to_string(), color)
        } else {
            (
                format!("{}/{}", props.current_index + 1, props.match_count),
                theme.text_muted(),
            )
        };
        draw_counter(ui, &counter_text, counter_color.into());

        // Prev / Next — 항상 렌더, 매치 없으면 disabled.
        let nav_enabled = props.match_count > 0;
        nav_button(ui, theme, CHEVRON_UP, nav_enabled);
        nav_button(ui, theme, CHEVRON_DOWN, nav_enabled);

        // Option toggles.
        toggle_button(ui, theme, "Aa", props.case_active);
        toggle_button(ui, theme, ".*", props.regex_active);
        toggle_button(ui, theme, "ab", props.word_active);

        // Divider (1px, separator), 좌우 size-2 margin.
        let (drect, _) = ui.allocate_exact_size(
            egui::vec2(theme.border_width.value(), ui.available_height()),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(
            drect,
            0.0,
            theme.overlay_hover().to_egui_premultiplied(),
        );

        // Close.
        nav_button_icon(ui, theme, CLOSE);
    });
}

/// 고정폭(40px) 매치 카운터를 가운데 정렬로 그린다. (본체 draw_counter 미러)
fn draw_counter(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(40.0, ui.available_height()), egui::Sense::hover());
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), color);
    let pos = rect.center() - galley.size() * 0.5;
    ui.painter().galley(pos, galley, color);
}

/// IconButton sm 정사각 프레임 + hover/active 오버레이. (본체 icon_button_frame 미러)
fn icon_button_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    enabled: bool,
    active: bool,
) -> (egui::Rect, egui::Response) {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let btn_size = theme.item_height_tab.value();
    let radius = theme.corner_radius.value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(btn_size, btn_size), sense);
    if active {
        ui.painter()
            .rect_filled(rect, radius, theme.overlay_active().to_egui_premultiplied());
    } else if enabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius, theme.overlay_hover().to_egui_premultiplied());
    }
    (rect, resp)
}

/// chevron IconButton sm. disabled 면 opacity_disabled 로 흐리게. (본체 nav_button 미러)
fn nav_button(ui: &mut egui::Ui, theme: &Theme, icon: MockIcon, enabled: bool) {
    let (rect, resp) = icon_button_frame(ui, theme, enabled, false);
    let color: egui::Color32 = if !enabled {
        egui::Color32::from(theme.text_secondary()).gamma_multiply(theme.opacity_disabled())
    } else if resp.hovered() {
        theme.text_primary().into()
    } else {
        theme.text_secondary().into()
    };
    let glyph = theme.icon_glyph_size_sm.value();
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    icon.image(glyph, color).paint_at(ui, icon_rect);
}

/// 항상 enabled 인 icon-only IconButton sm (close). (본체 close 버튼 미러)
fn nav_button_icon(ui: &mut egui::Ui, theme: &Theme, icon: MockIcon) {
    let (rect, resp) = icon_button_frame(ui, theme, true, false);
    let color: egui::Color32 = if resp.hovered() {
        theme.text_primary().into()
    } else {
        theme.text_secondary().into()
    };
    let glyph = theme.icon_glyph_size_sm.value();
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    icon.image(glyph, color).paint_at(ui, icon_rect);
}

/// mono 라벨 토글 IconButton sm. (본체 toggle_button 미러)
fn toggle_button(ui: &mut egui::Ui, theme: &Theme, label: &str, active: bool) {
    let (rect, _) = icon_button_frame(ui, theme, true, active);
    let color: egui::Color32 = if active {
        theme.text_primary().into()
    } else {
        theme.text_muted().into()
    };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), egui::FontId::monospace(11.0), color);
    let pos = rect.center() - galley.size() * 0.5;
    ui.painter().galley(pos, galley, color);
}

/// 검색바를 popup frame (surface-raised + border-strong + radius) 안에 그린다.
/// 디자인 canonical: padding 4, height 28 (item_height_tab 24 + padding 2*2).
fn with_bar_frame(ui: &mut egui::Ui, theme: &Theme, paint: impl FnOnce(&mut egui::Ui)) {
    let pad = 4.0;
    let body_h = theme.item_height_tab.value() + pad * 2.0;
    let (frame_rect, _) =
        ui.allocate_exact_size(egui::vec2(BAR_W, body_h), egui::Sense::hover());
    let painter = ui.painter_at(frame_rect);
    painter.rect_filled(
        frame_rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
    );
    painter.rect_stroke(
        frame_rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
    let inner = frame_rect.shrink(pad);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    paint(&mut child);
}

/// 대표 상태 5 종:
/// 1. 빈 query — placeholder, 카운터 0/0 muted, nav disabled
/// 2. 매치 있음 — 3/12, nav enabled
/// 3. 무매치 (쿼리 있음) — 0/0 danger
/// 4. 토글 전부 on — Aa/.*/ab active
/// 5. 정규식 에러 — danger 카운터 + regex active
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "draw_search_bar — 포커스 surface 우상단 headless 검색바 (360×28). 본체 view 의 시각 미러.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    let cases: &[(&str, MockProps)] = &[
        (
            "① 빈 query — placeholder, 카운터 0/0(muted), nav disabled:",
            MockProps {
                query: "",
                match_count: 0,
                current_index: 0,
                error: false,
                case_active: false,
                regex_active: false,
                word_active: false,
            },
        ),
        (
            "② 매치 있음 — \"fn \" 3/12, nav enabled:",
            MockProps {
                query: "fn ",
                match_count: 12,
                current_index: 2,
                error: false,
                case_active: false,
                regex_active: false,
                word_active: false,
            },
        ),
        (
            "③ 무매치 (쿼리 있음) — 0/0 danger:",
            MockProps {
                query: "zzqq",
                match_count: 0,
                current_index: 0,
                error: false,
                case_active: false,
                regex_active: false,
                word_active: false,
            },
        ),
        (
            "④ 토글 전부 on — Aa/.*/ab active, 매치 5/40:",
            MockProps {
                query: "Error",
                match_count: 40,
                current_index: 4,
                error: false,
                case_active: true,
                regex_active: true,
                word_active: true,
            },
        ),
        (
            "⑤ 정규식 에러 — regex active + 카운터 danger:",
            MockProps {
                query: "(unclosed",
                match_count: 0,
                current_index: 0,
                error: true,
                case_active: false,
                regex_active: true,
                word_active: false,
            },
        ),
    ];

    for (label, props) in cases {
        ui.label(egui::RichText::new(*label).color(egui::Color32::from(theme.text)));
        ui.add_space(4.0);
        with_bar_frame(ui, theme, |ui| draw_mock_bar(ui, theme, props));
        ui.add_space(16.0);
    }

    ui.label(
        egui::RichText::new(
            "⚠ 본체 view 와 시각 동기화. 키 입력 (Enter/↑↓/Esc) 은 view 내부 — 갤러리는 시각만.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
