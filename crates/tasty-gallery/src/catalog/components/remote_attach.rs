//! Add remote workspace — 680×460 two-pane 원격 워크스페이스 picker (NEW).
//!
//! 좌: tasty-attach 프로필 리스트(single select) → 우: 선택 프로필의 원격
//! 워크스페이스를 4상태로 표시(initial / connecting / error / loaded[+empty]).
//! 디자인 미러: `gallery/overlays-shared.jsx` `RemoteAttachFrame({state})` +
//! `ui_kits/terminal/overlays/remote_attach.jsx` `RemoteAttach`. remote_tool 과 같은
//! shell 언어(headless 헤더 · bg-panel 프레임 · ghost/primary footer).
//!
//! - `draw` = loaded 상태(원격 ws 리스트, 대형).
//! - `draw_states` = 비-list 3+1 상태(initial / connecting / error / empty).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Spinner, StatusKind, status_dot,
};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

// ── 프레임 고정 치수 (디자인 raw px — 화면 전용 고정값) ──
const FRAME_W: f32 = 680.0;
const FRAME_H: f32 = 460.0;
const LEFT_W: f32 = 240.0;
const HEADER_H: f32 = 47.0; // padding 10/10 + content 27
const HEADER_PAD_L: f32 = 14.0; // 디자인 L14 (size-14)
const FOOTER_H: f32 = 49.0;
const BODY_H: f32 = FRAME_H - HEADER_H - FOOTER_H;
const PROFILE_ROW_H: f32 = 50.0; // name(2 lines) + padding sm
const WS_ROW_H: f32 = 34.0;
const BADGE_H: f32 = 16.0; // design size-16

/// 우측 pane 상태.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RaState {
    Initial,
    Connecting,
    Error,
    Loaded,
    Empty,
}

struct Prof {
    name: &'static str,
    label: &'static str,
    target: &'static str,
    inactive: bool,
}

// 디자인 RA_ATTACHES seed 1:1 (overlays-shared.jsx).
const PROFILES: &[Prof] = &[
    Prof {
        name: "prod-web",
        label: "us-east",
        target: "deploy@10.0.4.12",
        inactive: false,
    },
    Prof {
        name: "gb10",
        label: "",
        target: "→ prod-web",
        inactive: false,
    },
    Prof {
        name: "edge-direct",
        label: "",
        target: "root@edge.example.com",
        inactive: false,
    },
    Prof {
        name: "media-nas",
        label: "lab",
        target: "→ nas.local",
        inactive: false,
    },
    Prof {
        name: "legacy-attach",
        label: "",
        target: "→ legacy-box",
        inactive: true,
    },
];

struct Ws {
    name: &'static str,
    panes: u32,
    busy: bool,
    attached: bool,
}

// 디자인 RA_WORKSPACES.t1 seed (prod-web).
const WORKSPACES: &[Ws] = &[
    Ws {
        name: "agents-prod",
        panes: 3,
        busy: true,
        attached: false,
    },
    Ws {
        name: "api-gateway",
        panes: 2,
        busy: false,
        attached: true,
    },
    Ws {
        name: "scratch",
        panes: 1,
        busy: false,
        attached: false,
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        ra_card(ui, theme, RaState::Loaded);
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "680×460 · bg-panel · headless header"),
            ("left", "240px bg-sidebar · attach profiles (single select)"),
            ("right", "flex · 4 states off left selection"),
            ("ws row", "StatusDot · name · panes · busy / in-use badge"),
            ("selected", "surface-active + 2px accent left bar"),
            ("footer", "Connect (primary, conditional) · Cancel (ghost)"),
        ],
        &[
            TokenChip::new("bg-sidebar", "left pane", theme.bg_sidebar().to_egui()),
            TokenChip::new(
                "surface-active",
                "selected row",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "select bar / Connect",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "accent-attached",
                "in-use badge (lavender)",
                theme.border_attached().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "The picker consumes the tasty-attach profiles edited in remote_tool — one store, \
         listed here. A remote workspace already attached elsewhere shows a lavender \
         'in use' badge and can't be selected (prevents a double-mirror.)",
    );
}

pub fn draw_states(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        ra_card(ui, theme, RaState::Initial);
        ra_card(ui, theme, RaState::Connecting);
        ra_card(ui, theme, RaState::Error);
        ra_card(ui, theme, RaState::Empty);
    });

    spec::meta(
        ui,
        theme,
        &[
            ("initial", "remote glyph + 'Select an attach profile'"),
            ("connecting", "Spinner + 'Connecting…' + SSH note"),
            ("error", "danger warn glyph + reason + Retry"),
            (
                "empty",
                "placeholder glyph + 'No workspaces on this remote'",
            ),
            ("center", "flex-centered, gap sm, padding xl/lg"),
        ],
        &[
            TokenChip::new(
                "accent-danger",
                "error glyph",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "text-placeholder",
                "initial/empty glyph",
                theme.text_placeholder().to_egui(),
            ),
            TokenChip::new("text-muted", "prompt copy", theme.text_muted().to_egui()),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Model all four states explicitly — the connect (SSH tunnel + list) can take \
         seconds; never leave the right pane blank while it resolves.",
    );
}

// ════════════════════════════════════════════════════════════════════════
/// 680×460 카드 한 장.
fn ra_card(ui: &mut egui::Ui, theme: &Theme, state: RaState) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: egui::Color32::from_black_alpha(120),
        })
        .show(ui, |ui| {
            ui.set_width(FRAME_W);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.vertical(|ui| {
                ui.set_width(FRAME_W);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                header(ui, theme);
                body(ui, theme, state);
                footer(ui, theme, state);
            });
        });
}

fn header(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(FRAME_W, HEADER_H), egui::Sense::hover());
    // borderBottom separator.
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    // 디자인 padding T10 R10 B10 L14.
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + HEADER_PAD_L,
            rect.top() + theme.spacing_sm.value(),
        ),
        egui::pos2(
            rect.right() - theme.spacing_sm.value(),
            rect.bottom() - theme.spacing_sm.value(),
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut child,
        icons::REMOTE,
        theme.icon_glyph_size_md.value(),
        theme.text_muted().to_egui(),
    );
    kit::title(&mut child, theme, "Add remote workspace");
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .show(ui, theme, &|ui, rect, c| {
                icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
            });
    });
}

fn body(ui: &mut egui::Ui, theme: &Theme, state: RaState) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(FRAME_W, BODY_H), egui::Sense::hover());
    let left = egui::Rect::from_min_size(rect.min, egui::vec2(LEFT_W, BODY_H));
    let right = egui::Rect::from_min_max(egui::pos2(rect.left() + LEFT_W, rect.top()), rect.max);
    // 좌 pane 배경(bg-sidebar) + borderRight.
    ui.painter()
        .rect_filled(left, 0.0, theme.bg_sidebar().to_egui());
    ui.painter().vline(
        left.right(),
        left.y_range(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    left_pane(ui, theme, left, state);
    right_pane(ui, theme, right, state);
}

fn left_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, state: RaState) {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    // caps 헤더 — padding 10/12/4.
    let hdr = egui::Rect::from_min_size(rect.min, egui::vec2(LEFT_W, 30.0));
    let (_, _) = col.allocate_exact_size(egui::vec2(LEFT_W, 30.0), egui::Sense::hover());
    col.painter().text(
        egui::pos2(
            hdr.left() + theme.spacing_md.value(),
            hdr.top() + theme.spacing_md.value(),
        ),
        egui::Align2::LEFT_TOP,
        "ATTACH PROFILES",
        egui::FontId::monospace(theme.font_size_micro.value()),
        theme.text_muted().to_egui(),
    );
    // 선택 규칙(디자인 미러): loaded → prod-web, error → legacy-attach, else prod-web.
    let sel_name = match state {
        RaState::Error => "legacy-attach",
        RaState::Connecting => "gb10",
        _ => "prod-web",
    };
    for p in PROFILES {
        profile_row(&mut col, theme, p, p.name == sel_name);
    }
}

fn profile_row(ui: &mut egui::Ui, theme: &Theme, p: &Prof, selected: bool) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, PROFILE_ROW_H), egui::Sense::hover());
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        // inset 2px accent 좌측바.
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height()));
        ui.painter()
            .rect_filled(bar, 0.0, theme.accent_primary().to_egui());
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + theme.spacing_md.value(),
            rect.top() + theme.spacing_sm.value(),
        ),
        egui::pos2(
            rect.right() - theme.spacing_md.value(),
            rect.bottom() - theme.spacing_sm.value(),
        ),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 2.0;
    child.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        let name_c = if selected {
            theme.text_primary()
        } else {
            theme.text_secondary()
        };
        ui.label(
            egui::RichText::new(p.name)
                .size(theme.font_size_body.value())
                .strong()
                .color(name_c.to_egui()),
        );
        if !p.label.is_empty() {
            ui.label(
                egui::RichText::new(format!("({})", p.label))
                    .size(theme.font_size_body.value())
                    .color(theme.text_muted().to_egui()),
            );
        }
        if p.inactive {
            badge(
                ui,
                theme,
                "inactive",
                theme.accent_warning().to_egui(),
                0.12,
                0.40,
                true,
            );
        }
    });
    child.label(
        egui::RichText::new(p.target)
            .monospace()
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

fn right_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, state: RaState) {
    match state {
        RaState::Loaded => loaded_pane(ui, theme, rect),
        RaState::Initial => center_state(
            ui,
            theme,
            rect,
            icons::REMOTE,
            theme.text_placeholder().to_egui(),
            false,
            "Select an attach profile",
            theme.text_muted(),
            "Pick a profile on the left to connect and list the remote instance's workspaces.",
            false,
        ),
        RaState::Connecting => center_state(
            ui,
            theme,
            rect,
            icons::REMOTE,
            theme.text_placeholder().to_egui(),
            true,
            "Connecting…",
            theme.text_secondary(),
            "Establishing the SSH tunnel to gb10 and listing workspaces. This can take a few seconds.",
            false,
        ),
        RaState::Error => center_state(
            ui,
            theme,
            rect,
            icons::ALERT_TRIANGLE,
            theme.accent_danger().to_egui(),
            false,
            "Can't connect",
            theme.text_primary(),
            "SSH authentication failed — passkey \u{201c}old-rsa\u{201d} was rejected by legacy-box.",
            true,
        ),
        RaState::Empty => center_state(
            ui,
            theme,
            rect,
            icons::FOLDER,
            theme.text_placeholder().to_egui(),
            false,
            "No workspaces on this remote",
            theme.text_muted(),
            "legacy-box is reachable but has no open workspaces to mirror.",
            false,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn center_state(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    glyph: MockGlyph,
    glyph_color: egui::Color32,
    spinner: bool,
    heading: &str,
    heading_color: tasty_type_appearance::color::HexColor,
    caption: &str,
    retry: bool,
) {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(
                theme.spacing_lg.value(),
                theme.spacing_xl.value(),
            )))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    // 세로 중앙 정렬 — 위쪽 여백을 대략 반으로.
    col.add_space((rect.height() - 120.0).max(0.0) * 0.5);
    col.spacing_mut().item_spacing.y = theme.spacing_sm.value();
    if spinner {
        Spinner::new().size(22.0).show(&mut col, theme);
    } else {
        kit::icon(&mut col, glyph, 22.0, glyph_color);
    }
    col.label(
        egui::RichText::new(heading)
            .size(theme.font_size_body.value())
            .strong()
            .color(heading_color.to_egui()),
    );
    col.label(
        egui::RichText::new(caption)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
    if retry {
        col.add_space(theme.spacing_xs.value());
        Button::new("Retry")
            .variant(ButtonVariant::Secondary)
            .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
            .show(&mut col, theme);
    }
}

fn loaded_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    // caps 헤더 — "Remote workspaces · prod-web".
    let (hdr, _) = col.allocate_exact_size(egui::vec2(rect.width(), 30.0), egui::Sense::hover());
    let base_x = hdr.left() + theme.spacing_md.value();
    let y = hdr.top() + theme.spacing_md.value();
    let caps = col.painter().layout_no_wrap(
        "REMOTE WORKSPACES".to_owned(),
        egui::FontId::monospace(theme.font_size_micro.value()),
        theme.text_muted().to_egui(),
    );
    let caps_w = caps.rect.width();
    col.painter()
        .galley(egui::pos2(base_x, y), caps, theme.text_muted().to_egui());
    col.painter().text(
        egui::pos2(base_x + caps_w + theme.spacing_sm.value(), y),
        egui::Align2::LEFT_TOP,
        "· prod-web",
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    for w in WORKSPACES {
        ws_row(&mut col, theme, w);
    }
}

fn ws_row(ui: &mut egui::Ui, theme: &Theme, w: &Ws) {
    let width = ui.available_width();
    let selected = w.name == "agents-prod"; // 디자인: 첫 행 selected.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, WS_ROW_H), egui::Sense::hover());
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height()));
        ui.painter()
            .rect_filled(bar, 0.0, theme.accent_primary().to_egui());
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + theme.spacing_md.value(), rect.top()),
        egui::pos2(rect.right() - theme.spacing_md.value(), rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    let kind = if w.busy {
        StatusKind::Running
    } else {
        StatusKind::Idle
    };
    status_dot(&mut child, theme, kind, "", w.busy, false);
    let name_c = if w.attached {
        theme.text_disabled()
    } else if selected {
        theme.text_primary()
    } else {
        theme.text_secondary()
    };
    child.label(
        egui::RichText::new(w.name)
            .size(theme.font_size_body.value())
            .color(name_c.to_egui()),
    );
    // panes 아이콘 + count.
    kit::icon(
        &mut child,
        icons::SPLIT,
        theme.font_size_caption.value(),
        theme.text_muted().to_egui(),
    );
    child.label(
        egui::RichText::new(w.panes.to_string())
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if w.attached {
            badge(
                ui,
                theme,
                "in use",
                theme.border_attached().to_egui(),
                0.14,
                0.45,
                false,
            );
        } else if w.busy {
            ui.label(
                egui::RichText::new("busy")
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        }
    });
}

/// 원격/inactive pill — fill/border alpha 는 디자인 color-mix(% transparent) 근사.
fn badge(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &str,
    color: egui::Color32,
    fill_a: f32,
    border_a: f32,
    warn_icon: bool,
) {
    let font = egui::FontId::monospace(theme.font_size_micro.value());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER);
    let pad_x = theme.spacing_sm.value();
    let icon_w = if warn_icon { 12.0 + 4.0 } else { 0.0 };
    let w = pad_x * 2.0 + icon_w + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, BADGE_H), egui::Sense::hover());
    let radius = theme.corner_radius_sm.value();
    ui.painter()
        .rect_filled(rect, radius, color.gamma_multiply(fill_a));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), color.gamma_multiply(border_a)),
        egui::StrokeKind::Inside,
    );
    let mut tx = rect.left() + pad_x;
    if warn_icon {
        let ir = egui::Rect::from_min_size(
            egui::pos2(tx, rect.center().y - 6.0),
            egui::vec2(12.0, 12.0),
        );
        icons::ALERT_TRIANGLE.image(12.0, color).paint_at(ui, ir);
        tx += 12.0 + 4.0;
    }
    ui.painter().galley(
        egui::pos2(tx, rect.center().y - galley.rect.height() * 0.5),
        galley,
        color,
    );
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: RaState) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(FRAME_W, FOOTER_H), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + theme.spacing_lg.value(), rect.top()),
        egui::pos2(rect.right() - theme.spacing_lg.value(), rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    // Connect 는 loaded + 선택 ws 있을 때만 활성 (specimen: loaded 에서만 활성).
    Button::new("Connect")
        .variant(ButtonVariant::Primary)
        .enabled(state == RaState::Loaded)
        .show(&mut child, theme);
    Button::new("Cancel")
        .variant(ButtonVariant::Ghost)
        .show(&mut child, theme);
}
