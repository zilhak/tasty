//! Icons 카탈로그 페이지 — 디자인(4) `gallery/icons.jsx` 의 `system-rules` Section
//! + 6 job 그룹 Section 미러. 루트 `icons.json` 의 canonical 글리프 단일 소스.
//!
//! 한 지오메트리(24×24 viewBox, 2px stroke, round cap/join, no fill, currentColor)
//! 를 job 별로 묶어 보여준다. 색은 글리프에 박지 않고 감싸는 컨트롤의 전경색을
//! 상속한다 — 갤러리 타일에서는 `theme.*` 토큰으로 tint 한다.
//!
//! 여기 정의된 `MockGlyph` 상수가 **유일 소스**다. primitive specimen 이 쓰던
//! `components/glyph.rs` 는 이 모듈에서 재노출만 한다(중복 path 정의 제거).
//!
//! 페이지 트리(Section/Spec 헤딩)는 `catalog.rs` 가, 각 Spec 본문은 여기의 draw
//! 함수가 그린다 — `draw_system_rules` + 6 그룹 draw(`draw_actions` 등).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{IconButton, Input};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, dont, meta, note, stage};

/// `<svg>` children 을 담은 글리프. `image()` 로 tint 된 egui 이미지를 만든다
/// (painter 클로저에서 `paint_at`). stroke 는 white 로 고정하고 `.tint(color)` 로
/// currentColor 를 재현한다.
#[derive(Clone, Copy)]
pub struct MockGlyph {
    uri: &'static str,
    svg: &'static str,
}

impl MockGlyph {
    /// `size` 정사각, `color` tint 의 egui 이미지.
    pub fn image(self, size: f32, color: egui::Color32) -> egui::Image<'static> {
        egui::Image::from_bytes(self.uri, self.svg.as_bytes())
            .fit_to_exact_size(egui::vec2(size, size))
            .tint(color)
    }
}

macro_rules! glyph {
    ($name:ident, $uri:literal, $body:literal) => {
        pub const $name: MockGlyph = MockGlyph {
            uri: concat!("bytes://gallery_icon_", $uri, ".svg"),
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
        };
    };
}

// ── Actions ──
glyph!(PLUS, "plus", r#"<path d="M12 5v14M5 12h14"/>"#);
glyph!(CLOSE, "close", r#"<path d="M18 6 6 18M6 6l12 12"/>"#);
glyph!(REFRESH, "refresh", r#"<path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6"/>"#);
glyph!(EDIT, "edit", r#"<path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"/>"#);
glyph!(TRASH, "trash", r#"<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#);
glyph!(COPY, "copy", r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/>"#);
glyph!(SEARCH, "search", r#"<circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>"#);

// ── Navigation & disclosure ──
glyph!(CHEVRON_RIGHT, "chevron_right", r#"<path d="m9 18 6-6-6-6"/>"#);
glyph!(CHEVRON_DOWN, "chevron_down", r#"<path d="m6 9 6 6 6-6"/>"#);
glyph!(CHEVRON_LEFT, "chevron_left", r#"<path d="m15 18-6-6 6-6"/>"#);
glyph!(CHEVRONS_LEFT, "chevrons_left", r#"<path d="m11 17-5-5 5-5M18 17l-5-5 5-5"/>"#);
glyph!(CHEVRONS_RIGHT, "chevrons_right", r#"<path d="m13 17 5-5-5-5M6 17l5-5-5-5"/>"#);

// ── Surfaces & workspace ──
glyph!(TERMINAL, "terminal", r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#);
glyph!(MARKDOWN, "markdown", r#"<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 15V9l2.5 3L12 9v6M16 9v4m0 0 2-2m-2 2-2-2"/>"#);
glyph!(SPLIT, "split", r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>"#);
glyph!(FOLDER, "folder", r#"<path d="M4 20h16a1 1 0 0 0 1-1V8a1 1 0 0 0-1-1h-7l-2-2H4a1 1 0 0 0-1 1v13a1 1 0 0 0 1 1z"/>"#);
glyph!(FILE, "file", r#"<path d="M14 3v4a1 1 0 0 0 1 1h4"/><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z"/>"#);
glyph!(REMOTE, "remote", r#"<path d="M4 17l6-6-6-6"/><path d="M12 19h8"/>"#);
glyph!(PORT, "port", r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v3m0 14v3M2 12h3m14 0h3"/>"#);

// ── Visibility ──
glyph!(EYE, "eye", r#"<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z"/><circle cx="12" cy="12" r="3"/>"#);
glyph!(EYE_OFF, "eye_off", r#"<path d="M9.9 4.2A10.9 10.9 0 0 1 12 4c6.5 0 10 7 10 7a18.5 18.5 0 0 1-2.2 3.2M6.6 6.6A18.5 18.5 0 0 0 2 11s3.5 7 10 7a10.9 10.9 0 0 0 4-.7M3 3l18 18"/>"#);

// ── Status & alerts ──
glyph!(ALERT_TRIANGLE, "alert_triangle", r#"<path d="M10.3 3.9 1.8 18a1 1 0 0 0 .9 1.5h18.6a1 1 0 0 0 .9-1.5L13.7 3.9a1 1 0 0 0-1.7 0z"/><path d="M12 9v4M12 17h.01"/>"#);
glyph!(ALERT_CIRCLE, "alert_circle", r#"<circle cx="12" cy="12" r="9"/><path d="M12 8v4m0 4h.01"/>"#);
glyph!(SHIELD_CHECK, "shield_check", r#"<path d="M12 3l7 3v6c0 4-3 6.5-7 9-4-2.5-7-5-7-9V6z"/><path d="M9 12l2 2 4-4"/>"#);
glyph!(BELL, "bell", r#"<path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 0 1-3.4 0"/>"#);

// ── Tools & system ──
glyph!(TOOLS, "tools", r#"<path d="M14.7 6.3a4 4 0 0 1-5.4 5.4L4 17v3h3l5.3-5.3a4 4 0 0 1 5.4-5.4l-2.7 2.7-2-2 2.7-2.7z"/>"#);
glyph!(SETTINGS, "settings", r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#);
glyph!(PLUG, "plug", r#"<path d="M9 2v6M15 2v6M7 8h10v3a5 5 0 0 1-10 0V8zM12 16v6"/>"#);
glyph!(ROCKET, "rocket", r#"<path d="M5 13c-1.5 1.5-2 5-2 5s3.5-.5 5-2a3.5 3.5 0 1 0-3-3zM12 15l-3-3a14 14 0 0 1 9-9 14 14 0 0 1-3 9zM9 12l3 3"/>"#);

/// 한 글리프 카탈로그 항목: (글리프, canonical name, role).
type Entry = (MockGlyph, &'static str, &'static str);

// ── job 그룹별 글리프 슬라이스 (icons.jsx GROUPS 순서/구성 미러) ──

const ACTIONS: &[Entry] = &[
    (PLUS, "plus", "create / add"),
    (CLOSE, "close", "dismiss overlay (header X)"),
    (REFRESH, "refresh", "re-scan / reload"),
    (EDIT, "edit", "edit a row / value"),
    (TRASH, "trash", "delete / remove"),
    (COPY, "copy", "copy to clipboard"),
    (SEARCH, "search", "filter / search affordance"),
];

const NAV: &[Entry] = &[
    (CHEVRON_RIGHT, "chevronRight", "collapsed row · forward"),
    (CHEVRON_DOWN, "chevronDown", "expanded row"),
    (CHEVRON_LEFT, "chevronLeft", "back"),
    (CHEVRONS_LEFT, "chevronsLeft", "collapse sidebar"),
    (CHEVRONS_RIGHT, "chevronsRight", "expand sidebar rail"),
];

const SURFACES: &[Entry] = &[
    (TERMINAL, "terminal", "terminal surface / tab"),
    (MARKDOWN, "markdown", "markdown surface / tab"),
    (SPLIT, "split", "split a pane"),
    (FOLDER, "folder", "folder / workspace"),
    (FILE, "file", "file leaf"),
    (REMOTE, "remote", "remote connection"),
    (PORT, "port", "listening port / target"),
];

const VISIBILITY: &[Entry] = &[
    (EYE, "eye", "reveal value"),
    (EYE_OFF, "eyeOff", "hide value"),
];

const STATUS: &[Entry] = &[
    (ALERT_TRIANGLE, "alertTriangle", "warning / unverified"),
    (ALERT_CIRCLE, "alertCircle", "error / failed"),
    (SHIELD_CHECK, "shieldCheck", "trusted / signed"),
    (BELL, "bell", "notification"),
];

const SYSTEM: &[Entry] = &[
    (TOOLS, "tools", "Tools menu"),
    (SETTINGS, "settings", "Settings window"),
    (PLUG, "plug", "Plugins"),
    (ROCKET, "rocket", "getting started / launch"),
];

// ── icongrid 타일 치수 (icons.jsx `.icongrid` / `.icontile`) ──
//
// Theme 에 대응 토큰이 없는 카탈로그 그리드 전용 치수 — 디자인 px 를 주석으로 명시.
/// `.icongrid` auto-fill `minmax(132px, 1fr)` 의 셀 폭.
const TILE_W: f32 = 132.0;
/// `.icontile` 높이 — padding 18/13 + glyph-box 36 + name/role.
const TILE_H: f32 = 110.0;
/// `.icontile .glyph` 박스 36×36 안에 그리는 글리프 — jsx `<GIcon size={22}>`.
const TILE_GLYPH: f32 = 22.0;
/// `.glyph` 박스 한 변 — 글리프 수직 중심 산출용.
const GLYPH_BOX: f32 = 36.0;

thread_local! {
    /// system-rules 데모의 filter Input 버퍼 (egui memory 에 포커스 유지).
    static FILTER_BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

#[inline]
fn ec(c: impl Into<egui::Color32>) -> egui::Color32 {
    c.into()
}

// ── system-rules Section ──────────────────────────────────────────────

/// 아이콘 시스템 규칙 — size 스케일 / currentColor / IconButton 데모 + meta.
pub fn draw_system_rules(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Wrap, |ui| {
        cluster(ui, theme, "size scale — set via size prop", |ui| {
            // 26 / 20 / 16(default) / 14 / 12 — 데모 대상이 곧 size 스케일이라
            // 직접 값을 쓴다(prim_spinner 전례). 16 = icon-glyph-size-md.
            for s in [26.0_f32, 20.0, 16.0, 14.0, 12.0] {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(26.0, 26.0),
                        egui::Sense::hover(),
                    );
                    paint_glyph(ui, PORT, rect.center(), s, ec(theme.text_secondary()));
                    ui.label(
                        egui::RichText::new(format!("{s:.0}px"))
                            .monospace()
                            .size(theme.font_size_micro.value())
                            .color(ec(theme.text_muted())),
                    );
                });
            }
        });
        cluster(ui, theme, "inherits currentColor", |ui| {
            let size = theme.icon_glyph_size_md.value() + theme.spacing_xs.value(); // 16+4≈18
            let tints: [(MockGlyph, egui::Color32); 4] = [
                (REFRESH, ec(theme.text_muted())),
                (ALERT_TRIANGLE, ec(theme.accent_warning())),
                (SHIELD_CHECK, ec(theme.accent_success())),
                (ALERT_CIRCLE, ec(theme.accent_danger())),
            ];
            for (g, color) in tints {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(size, size),
                    egui::Sense::hover(),
                );
                paint_glyph(ui, g, rect.center(), size, color);
            }
        });
        cluster(ui, theme, "in an IconButton — the common case", |ui| {
            IconButton::new().show(ui, theme, &|ui, rect, c| {
                REFRESH.image(rect.height(), c).paint_at(ui, rect)
            });
            IconButton::new().show(ui, theme, &|ui, rect, c| {
                CLOSE.image(rect.height(), c).paint_at(ui, rect)
            });
            FILTER_BUF.with(|b| {
                let mut buf = b.borrow_mut();
                Input::new()
                    .placeholder("Filter…")
                    .width(theme.field_width_md.value())
                    .icon(&|ui, rect, c| SEARCH.image(rect.height(), c).paint_at(ui, rect))
                    .show(ui, theme, &mut buf);
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("viewBox", "0 0 24 24"),
            ("stroke", "2px · round cap + join"),
            ("fill", "none — stroke only"),
            ("color", "inherits currentColor"),
            ("sizes", "26 / 20 / 16 (default) / 14 / 12"),
            ("in IconButton", "28px square control-height"),
        ],
        &[
            TokenChip::new("text-muted", "rest glyph", ec(theme.text_muted())),
            TokenChip::new("accent-warning", "warn tint", ec(theme.accent_warning())),
            TokenChip::new("accent-success", "ok tint", ec(theme.accent_success())),
            TokenChip::new("accent-danger", "error tint", ec(theme.accent_danger())),
        ],
    );
    note(
        ui,
        theme,
        "This page is the single source for these glyphs. A new overlay's header X, \
         refresh, and filter-search all pull close / refresh / search from here — \
         never re-inline a <path>.",
    );
    dont(
        ui,
        theme,
        "Don't bake a color into a glyph, mix stroke widths, or add a fill. A filled \
         state indicator is StatusDot / Badge, not an icon.",
    );
}

// ── 6 job 그룹 Section — 각자 icongrid 만 그린다 ──────────────────────

pub fn draw_actions(ui: &mut egui::Ui, theme: &Theme) {
    icongrid(ui, theme, ACTIONS);
}
pub fn draw_nav(ui: &mut egui::Ui, theme: &Theme) {
    icongrid(ui, theme, NAV);
}
pub fn draw_surfaces(ui: &mut egui::Ui, theme: &Theme) {
    icongrid(ui, theme, SURFACES);
}
pub fn draw_visibility(ui: &mut egui::Ui, theme: &Theme) {
    icongrid(ui, theme, VISIBILITY);
}
pub fn draw_status(ui: &mut egui::Ui, theme: &Theme) {
    icongrid(ui, theme, STATUS);
}
pub fn draw_system(ui: &mut egui::Ui, theme: &Theme) {
    icongrid(ui, theme, SYSTEM);
}

/// 글리프 타일 그리드 — separator 배경 위에 1px gap 으로 panel 셀을 깐다
/// (`.icongrid` gap 1px on `--tasty-separator`, solo stage padding 0).
fn icongrid(ui: &mut egui::Ui, theme: &Theme, icons: &[Entry]) {
    egui::Frame::new()
        .fill(ec(theme.separator))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            ec(theme.border_default()),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                // gap 1px → 사이로 separator 배경이 비쳐 hairline 격자.
                ui.spacing_mut().item_spacing =
                    egui::vec2(theme.border_width.value(), theme.border_width.value());
                for (g, name, role) in icons {
                    tile(ui, theme, *g, name, role);
                }
            });
        });
}

fn tile(ui: &mut egui::Ui, theme: &Theme, g: MockGlyph, name: &str, role: &str) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // 셀 배경 — panel, hover 시 overlay-hover (web .icontile:hover).
    let bg = if resp.hovered() {
        ec(theme.overlay_hover())
    } else {
        ec(theme.bg_panel())
    };
    painter.rect_filled(rect, 0.0, bg);

    // 글리프 — padding-top 18 + glyph-box 36 중심.
    let glyph_cy = rect.top() + theme.spacing_lg.value() + 2.0 + GLYPH_BOX / 2.0;
    // hover 시 글리프도 secondary→primary 로 (web .icontile:hover .glyph).
    let glyph_color = if resp.hovered() {
        ec(theme.text_primary())
    } else {
        ec(theme.text_secondary())
    };
    paint_glyph(
        ui,
        g,
        egui::pos2(rect.center().x, glyph_cy),
        TILE_GLYPH,
        glyph_color,
    );

    // name (mono 12 primary) + role (micro muted).
    let name_y = glyph_cy + GLYPH_BOX / 2.0 + theme.spacing_sm.value();
    painter.text(
        egui::pos2(rect.center().x, name_y),
        egui::Align2::CENTER_TOP,
        name,
        egui::FontId::monospace(theme.font_size_term_sm.value()),
        ec(theme.text_primary()),
    );
    painter.text(
        egui::pos2(rect.center().x, name_y + theme.spacing_lg.value()),
        egui::Align2::CENTER_TOP,
        role,
        egui::FontId::proportional(theme.font_size_micro.value()),
        ec(theme.text_muted()),
    );

    resp.on_hover_text(role);
}

/// `center` 를 중심으로 한 `size` 정사각에 글리프를 `color` tint 로 그린다.
fn paint_glyph(ui: &mut egui::Ui, g: MockGlyph, center: egui::Pos2, size: f32, color: egui::Color32) {
    let r = egui::Rect::from_center_size(center, egui::vec2(size, size));
    g.image(size, color).paint_at(ui, r);
}
