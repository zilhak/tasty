//! Icons 카탈로그 페이지 — 디자인 `gallery/icons.jsx` + 루트 `icons.json` 의
//! canonical 29 글리프 단일 소스.
//!
//! 한 지오메트리(24×24 viewBox, 2px stroke, round cap/join, no fill, currentColor)
//! 를 job 별로 묶어 보여준다. 색은 글리프에 박지 않고 감싸는 컨트롤의 전경색을
//! 상속한다 — 갤러리 타일에서는 `theme.*` 토큰으로 tint 한다.
//!
//! 여기 정의된 `MockGlyph` 상수가 **유일 소스**다. primitive specimen 이 쓰던
//! `components/glyph.rs` 는 이 모듈에서 재노출만 한다(중복 path 정의 제거).

use tasty_type_appearance::theme::Theme;

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

/// job 별 그룹 — `icons.jsx` GROUPS 순서/구성 미러.
struct Group {
    title: &'static str,
    when: &'static str,
    icons: &'static [Entry],
}

const GROUPS: &[Group] = &[
    Group {
        title: "Actions",
        when: "The verbs — what an IconButton wraps in a toolbar or row. close/refresh sit in every overlay header.",
        icons: &[
            (PLUS, "plus", "create / add"),
            (CLOSE, "close", "dismiss overlay (header X)"),
            (REFRESH, "refresh", "re-scan / reload"),
            (EDIT, "edit", "edit a row / value"),
            (TRASH, "trash", "delete / remove"),
            (COPY, "copy", "copy to clipboard"),
            (SEARCH, "search", "filter / search affordance"),
        ],
    },
    Group {
        title: "Navigation & disclosure",
        when: "Movement and open/closed state. Single chevrons = tree-row disclosure; doubled = collapse/expand the sidebar rail.",
        icons: &[
            (CHEVRON_RIGHT, "chevronRight", "collapsed row · forward"),
            (CHEVRON_DOWN, "chevronDown", "expanded row"),
            (CHEVRON_LEFT, "chevronLeft", "back"),
            (CHEVRONS_LEFT, "chevronsLeft", "collapse sidebar"),
            (CHEVRONS_RIGHT, "chevronsRight", "expand sidebar rail"),
        ],
    },
    Group {
        title: "Surfaces & workspace",
        when: "The nouns of the workspace — what a tab, tree row, or new-surface button shows. terminal/markdown are the two core surface kinds.",
        icons: &[
            (TERMINAL, "terminal", "terminal surface / tab"),
            (MARKDOWN, "markdown", "markdown surface / tab"),
            (SPLIT, "split", "split a pane"),
            (FOLDER, "folder", "folder / workspace"),
            (FILE, "file", "file leaf"),
            (REMOTE, "remote", "remote connection"),
            (PORT, "port", "listening port / target"),
        ],
    },
    Group {
        title: "Visibility",
        when: "The reveal toggle on secret values (passkeys, env). eye when hidden, eyeOff when shown.",
        icons: &[
            (EYE, "eye", "reveal value"),
            (EYE_OFF, "eyeOff", "hide value"),
        ],
    },
    Group {
        title: "Status & alerts",
        when: "Inline meaning markers, tinted by the line they sit in (warning amber, success green, danger red) via currentColor.",
        icons: &[
            (ALERT_TRIANGLE, "alertTriangle", "warning / unverified"),
            (ALERT_CIRCLE, "alertCircle", "error / failed"),
            (SHIELD_CHECK, "shieldCheck", "trusted / signed"),
            (BELL, "bell", "notification"),
        ],
    },
    Group {
        title: "Tools & system",
        when: "The sidebar footer and global tools — each anchors a menu or window.",
        icons: &[
            (TOOLS, "tools", "Tools menu"),
            (SETTINGS, "settings", "Settings window"),
            (PLUG, "plug", "Plugins"),
            (ROCKET, "rocket", "getting started / launch"),
        ],
    },
];

/// 타일 크기 — `icons.jsx` `.icongrid` auto-fill `minmax(132px, 1fr)` 대응.
const TILE_W: f32 = 132.0;
const TILE_H: f32 = 88.0;
/// 타일 안 글리프 크기 — jsx `<GIcon size={22}>`.
const TILE_GLYPH: f32 = 22.0;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            icon_system(ui, theme);
            for group in GROUPS {
                section(ui, theme, group.title, group.when);
                grid(ui, theme, group.icons);
            }
        });
}

/// 상단 "icon system" — 불변식 + size 스케일 + currentColor 데모.
fn icon_system(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("The icon system")
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    note(
        ui,
        theme,
        "Every glyph: 24×24 viewBox, 2px stroke, round cap + join, no fill, currentColor. \
         An icon never carries its own color — it inherits from the control around it. \
         Size is set via a size prop (26 / 20 / 16 default / 14 / 12), not by editing the path.",
    );

    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("size scale — set via size prop")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        for s in [26.0_f32, 20.0, 16.0, 14.0, 12.0] {
            ui.vertical(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                paint_glyph(ui, PORT, rect.center(), s, egui::Color32::from(theme.subtext1));
                ui.label(
                    egui::RichText::new(format!("{s:.0}px"))
                        .monospace()
                        .size(theme.font_size_micro.value())
                        .color(egui::Color32::from(theme.subtext0)),
                );
            });
        }
    });

    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("inherits currentColor — same glyph, tinted by context")
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        let tints: [(MockGlyph, egui::Color32); 4] = [
            (REFRESH, egui::Color32::from(theme.subtext0)),
            (ALERT_TRIANGLE, egui::Color32::from(theme.accent_warning())),
            (SHIELD_CHECK, egui::Color32::from(theme.accent_success())),
            (ALERT_CIRCLE, egui::Color32::from(theme.accent_danger())),
        ];
        for (g, color) in tints {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            paint_glyph(ui, g, rect.center(), 18.0, color);
        }
    });
}

fn section(ui: &mut egui::Ui, theme: &Theme, title: &str, when: &str) {
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(title)
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    note(ui, theme, when);
    ui.add_space(6.0);
}

fn note(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .small()
            .italics()
            .color(egui::Color32::from(theme.subtext0)),
    );
}

/// 타일 그리드 — 가용 너비에 맞춰 wrap.
fn grid(ui: &mut egui::Ui, theme: &Theme, icons: &[Entry]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        for (g, name, role) in icons {
            tile(ui, theme, *g, name, role);
        }
    });
}

fn tile(ui: &mut egui::Ui, theme: &Theme, g: MockGlyph, name: &str, role: &str) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // 타일 배경 — 1px 보더, hover 시 옅은 강조(웹 .icontile hover tint 대응).
    let bg = if resp.hovered() {
        egui::Color32::from(theme.surface1)
    } else {
        egui::Color32::from(theme.surface0)
    };
    painter.rect_filled(rect, theme.corner_radius_sm.value(), bg);
    painter.rect_stroke(
        rect,
        theme.corner_radius_sm.value(),
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.surface2),
        ),
        egui::StrokeKind::Inside,
    );

    // 글리프 — 상단 가운데.
    let glyph_center = egui::pos2(rect.center().x, rect.top() + 22.0);
    paint_glyph(
        ui,
        g,
        glyph_center,
        TILE_GLYPH,
        egui::Color32::from(theme.subtext1),
    );

    // name (caption) + role (micro).
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 44.0),
        egui::Align2::CENTER_TOP,
        name,
        egui::FontId::proportional(theme.font_size_caption.value()),
        egui::Color32::from(theme.text),
    );
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 62.0),
        egui::Align2::CENTER_TOP,
        role,
        egui::FontId::proportional(theme.font_size_micro.value()),
        egui::Color32::from(theme.subtext0),
    );

    resp.on_hover_text(role);
}

/// `center` 를 중심으로 한 `size` 정사각에 글리프를 `color` tint 로 그린다.
fn paint_glyph(ui: &mut egui::Ui, g: MockGlyph, center: egui::Pos2, size: f32, color: egui::Color32) {
    let r = egui::Rect::from_center_size(center, egui::vec2(size, size));
    g.image(size, color).paint_at(ui, r);
}
