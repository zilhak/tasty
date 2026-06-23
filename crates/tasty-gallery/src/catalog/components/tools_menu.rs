//! Tools menu overlay 데모 (Overlays).
//!
//! 본체 `src/adapters/ui/tools_menu.rs::draw_tools_menu` 가 표현하는 시각을
//! 로컬 mock 으로 재현. 디자인 canonical: `overlays/tools_menu.jsx` (160px).
//!
//! 본체 의존: 0. gallery 가 binary crate `tasty` 에 의존 불가하므로 view 의
//! 시각 layout 만 복제하고 항목 목록은 로컬 mock 으로 주입한다. 본체 view
//! 변경 시 시각 동기화는 수동 검증.
//!
//! 160px 폭 popup. 28px 행, 아이콘 없음. 빌트인 항목 → separator → plugin
//! 기여 항목 순. 행 hover 시 overlay-hover 배경 + 텍스트 text(primary),
//! 비호버는 subtext0. 좌측 패딩 8px, body 폰트.

use tasty_type_appearance::theme::Theme;

/// 디자인 canonical 폭 (tools_menu.jsx: `width: 160`).
const POPUP_W: f32 = 160.0;
/// 한 행 높이 (본체 `ITEM_HEIGHT`).
const ITEM_HEIGHT: f32 = 28.0;
/// 본체 행 좌측 텍스트 패딩 (본체 draw 의 `rect.min.x + 8.0`).
const ROW_PAD_X: f32 = 8.0;

/// 한 행을 그린다. hover 면 overlay-hover 배경 + text, 아니면 subtext0.
/// (본체 draw_tools_menu 의 빌트인/플러그인 행 렌더 미러)
fn draw_row(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ITEM_HEIGHT), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, theme.overlay_hover().to_egui_premultiplied());
    }
    ui.painter().text(
        egui::pos2(rect.min.x + ROW_PAD_X, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_body.value()),
        if resp.hovered() {
            theme.text.into()
        } else {
            theme.subtext0.into()
        },
    );
}

/// 빌트인 ↔ 플러그인 항목 사이 separator. 본체는 `ui.separator()` 사용 —
/// egui 기본 separator 와 동일하게 1px separator 색 + 상하 spacing_xs.
fn draw_separator(ui: &mut egui::Ui, theme: &Theme) {
    let xs = theme.spacing_xs.value();
    ui.add_space(xs);
    let r = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().hline(
        r.x_range(),
        y,
        egui::Stroke::new(theme.border_width.value(), theme.overlay_hover().to_egui_premultiplied()),
    );
    ui.add_space(xs);
}

/// 160px popup frame (surface-raised + border-strong + radius) 안에 행 목록을 그린다.
/// 내부 패딩 = spacing_sm (본체 popup content margin 에 대응).
fn with_popup_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    builtin: &[&str],
    plugin: &[&str],
    paint: impl Fn(&mut egui::Ui, &Theme),
) {
    let pad = theme.spacing_sm.value();
    let xs = theme.spacing_xs.value();
    // content height = N·행 + (N-1)·item_spacing + (separator 시 1px + 상하 xs).
    let total = builtin.len() + plugin.len();
    let item_spacing = xs; // tasty_egui_theme 가 item_spacing.y 로 적용하는 값.
    let mut content_h = total as f32 * ITEM_HEIGHT
        + total.saturating_sub(1) as f32 * item_spacing;
    if !builtin.is_empty() && !plugin.is_empty() {
        content_h += theme.border_width.value() + xs * 2.0 + item_spacing;
    }
    let body_h = content_h + pad * 2.0;

    let (frame_rect, _) =
        ui.allocate_exact_size(egui::vec2(POPUP_W, body_h), egui::Sense::hover());
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
    child.spacing_mut().item_spacing.y = item_spacing;
    paint(&mut child, theme);
}

/// 대표 상태 3 종:
/// 1. 빌트인 + plugin (디자인 canonical: 5 빌트인 + 2 plugin, separator)
/// 2. 빌트인만 (plugin grant 없음 — separator 없음)
/// 3. plugin 항목 다수 (긴 라벨 clip 확인)
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "draw_tools_menu — 사이드바 도구 버튼 위 headless 메뉴 (160px). 본체 view 의 시각 미러.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    // 본체 BUILTIN_TOOLS 라벨 (lang/en.toml 의 tools_menu_item 값) 미러.
    let builtin: &[&str] = &[
        "Command palette…",
        "Listening ports...",
        "Remote connections…",
        "Check for updates…",
        "Presets",
    ];
    // plugin 기여 항목 (Clipboard History = builtin plugin, Git = git-viewer).
    let plugin: &[&str] = &["Clipboard History", "Git"];

    ui.label(
        egui::RichText::new("① 빌트인 5 + plugin 2 — separator (디자인 canonical):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    with_popup_frame(ui, theme, builtin, plugin, |ui, theme| {
        for label in builtin {
            draw_row(ui, theme, label);
        }
        draw_separator(ui, theme);
        for label in plugin {
            draw_row(ui, theme, label);
        }
    });
    ui.add_space(16.0);

    ui.label(
        egui::RichText::new("② 빌트인만 (plugin grant 없음) — separator 없음:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    with_popup_frame(ui, theme, builtin, &[], |ui, theme| {
        for label in builtin {
            draw_row(ui, theme, label);
        }
    });
    ui.add_space(16.0);

    let plugin_many: &[&str] = &[
        "Clipboard History",
        "Git",
        "Extremely long plugin tool label that should clip",
        "Task Runner",
    ];
    ui.label(
        egui::RichText::new("③ plugin 항목 다수 + 긴 라벨 clip:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    with_popup_frame(ui, theme, builtin, plugin_many, |ui, theme| {
        for label in builtin {
            draw_row(ui, theme, label);
        }
        draw_separator(ui, theme);
        for label in plugin_many {
            draw_row(ui, theme, label);
        }
    });

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "⚠ 본체 view 와 시각 동기화. 클릭 dispatch / Escape 닫기는 view 내부 — 갤러리는 시각만.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
