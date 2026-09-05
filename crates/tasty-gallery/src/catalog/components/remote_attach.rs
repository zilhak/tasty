//! Add remote workspace — 680×460 two-pane 원격 워크스페이스 picker (NEW).
//!
//! 좌: tasty-attach 프로필 리스트(single select) → 우: 선택 프로필의 원격
//! 워크스페이스를 4상태로 표시(initial / connecting / error / loaded[+empty]).
//! 디자인 미러: `gallery/overlays-shared.jsx` `RemoteAttachFrame({state})` +
//! `ui_kits/terminal/overlays/remote_attach.jsx` `RemoteAttach`. remote_tool 과 같은
//! shell 언어(headless 헤더 · bg-panel 프레임 · ghost/primary footer).
//!
//! - `draw` = loaded 상태(원격 ws 리스트 + "+ New workspace" 첫 행, 대형).
//! - `draw_states` = 비-list 3상태(initial / connecting / error) + empty(목록 경로).
//! - `draw_new_row` = "+ New workspace" 행 5상태(rest / hover / selected / creating / failed).
//!
//! 우측 pane 의 loaded 렌더 경로는 **하나**다 — 원격에 ws 가 0개여도 caps 헤더와
//! "+ New workspace" 행은 그대로 나오고 그 아래 muted 한 줄만 붙는다. 그래서 empty 는
//! 막다른 center-state 가 아니라 "행이 정확히 하나인 목록"으로 degrade 한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::{
    CENTER_BLOCK_H_SPECIMEN as EMPTY_BLOCK_H, CENTER_GLYPH_SIZE as EMPTY_GLYPH, STRUCT_GAP_2,
};
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Spinner, StatusKind,
    status_dot,
};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

// ── 프레임 고정 치수 (디자인 raw px — 화면 전용 고정값) ──
const FRAME_W: LogicalPx = LogicalPx(680.0);
const FRAME_H: f32 = 460.0;
const LEFT_W: f32 = 240.0;
const HEADER_H: f32 = 47.0; // padding 10/10 + content 27
const HEADER_PAD_L: f32 = 14.0; // 디자인 L14 (size-14)
const FOOTER_H: f32 = 49.0;
const BODY_H: LogicalPx = LogicalPx(FRAME_H - HEADER_H - FOOTER_H);
const PROFILE_ROW_H: LogicalPx = LogicalPx(50.0); // name(2 lines) + padding sm
const WS_ROW_H: LogicalPx = LogicalPx(34.0);
const BADGE_H: LogicalPx = LogicalPx(16.0); // design size-16
const STRIP_W: LogicalPx = LogicalPx(440.0); // 새 행 상태 specimen 의 pane 폭

/// 두 열의 caps 헤더("ATTACH PROFILES" / "REMOTE WORKSPACES") 행 높이 —
/// padding 12/12 + micro caps 한 줄. 4px 그리드 밖이고 대응 Theme 토큰이 없다.
const CAPS_HEADER_H: LogicalPx = LogicalPx(30.0);

/// 우측 pane 상태. `Loaded` / `Empty` 는 같은 목록 렌더 경로를 타고 ws 목록의
/// 길이만 다르다.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RaState {
    Initial,
    Connecting,
    Error,
    Loaded,
    Empty,
}

/// "+ New workspace" 행의 시각 상태 — 디자인 `RaNewWsRow` 의 `phase`(rest/creating/
/// failed)에 포인터/선택 상태를 합친 것. 이 행은 버튼이 아니라 **목록 행**이라
/// 이웃 ws 행과 같은 select-then-confirm 계약을 따른다.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NewRow {
    Rest,
    Hover,
    Selected,
    Creating,
    Failed,
}

impl NewRow {
    fn creating(self) -> bool {
        self == NewRow::Creating
    }
    fn failed(self) -> bool {
        self == NewRow::Failed
    }
    fn selected(self) -> bool {
        self == NewRow::Selected
    }
}

/// 생성 실패 시 행 하단에 인라인으로 붙는 원격 메시지(specimen seed).
const CREATE_ERROR: &str = "The remote refused workspace.create — the instance is read-only.";

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
            (
                "new row",
                "first row · plus glyph in dot slot · accent label · 'on remote'",
            ),
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
                "new-row glyph+label / select bar / Connect",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "separator",
                "1px rule closing the new-row group",
                theme.separator.to_egui(),
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

    spec::note(
        ui,
        theme,
        "'+ New workspace' is a list row, not a button: it flows through the same \
         single-select state as every remote workspace, so confirming it goes through the \
         footer like any other choice. The footer button then reads 'Create & connect' — \
         it has to say which of the two things it will do. It creates the workspace with \
         the remote's own default name and cwd; nothing is asked for.",
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
            (
                "connecting",
                "Spinner + 'Connecting…' + SSH note; footer ghost becomes Stop",
            ),
            ("error", "danger warn glyph + reason + Retry"),
            (
                "empty",
                "list path — caps header + new row (pre-selected) + one muted line",
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
                "initial glyph",
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

    spec::note(
        ui,
        theme,
        "Connecting is time-bounded: an unresponsive host would otherwise hold the pane for \
         minutes, so the lookup gives up after 20s and lands on the error state (with Retry). \
         The footer ghost button reads Stop while connecting and aborts the lookup back to \
         initial — it does not close the picker.",
    );

    spec::dont(
        ui,
        theme,
        "Don't give the empty remote its own centered state with a create button. That \
         would make the same action confirm two different ways — a button that fires on \
         click when the remote is empty, a row that waits for the footer when it isn't — \
         off a condition the user can't see coming. Empty is the same list with one row in \
         it, and that row starts selected so the footer is live the moment the pane paints.",
    );
}

/// "+ New workspace" 행 5상태 — 440px pane 폭 스트립(디자인 specimen 과 동일 폭).
pub fn draw_new_row(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for (label, state) in [
            ("rest", NewRow::Rest),
            ("hover", NewRow::Hover),
            ("selected", NewRow::Selected),
            ("creating", NewRow::Creating),
            ("failed", NewRow::Failed),
        ] {
            spec::cluster(ui, theme, label, |ui| {
                new_row_strip(ui, theme, state);
            });
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("box", "34px — the same row as a remote workspace"),
            (
                "glyph",
                "plus 14px centered in a status-dot-width slot (overflows both sides)",
            ),
            ("label", "13px / 500 — reads as a peer of the rows below"),
            (
                "rest · hover",
                "accent-primary label on panel / overlay-hover",
            ),
            (
                "selected",
                "text-primary on surface-active + 2px accent bar",
            ),
            (
                "creating",
                "Spinner + muted 'Creating workspace…'; list dims",
            ),
            ("failed", "danger warn glyph + inline reason + Try again"),
            ("group", "1px separator below, xs margin above and below"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "rest/hover glyph + label",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "overlay-hover",
                "hover fill",
                theme.overlay_hover().to_egui(),
            ),
            TokenChip::new(
                "surface-active",
                "selected fill",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "failed glyph + reason",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "text-muted",
                "'on remote' caption / creating label",
                theme.text_muted().to_egui(),
            ),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Keep the glyph inside a status-dot-width slot. The dot the workspace rows draw is \
         8px and the plus is 14px, so the plus overflows its slot symmetrically and the name \
         column starts on exactly the same pixel as every row beneath it.",
    );

    spec::note(
        ui,
        theme,
        "Selected drops the accent label for text-primary. Accent on the active surface \
         measures 3.17:1 — the row would be least readable at the moment it is chosen. The \
         row still reads as the odd one out through the glyph, the separator, and the accent \
         bar, so nothing is lost by letting the label go quiet.",
    );

    spec::note(
        ui,
        theme,
        "Creating dims the list below it rather than replacing the pane with a spinner — the \
         round trip is a second or two and the user was reading that list. Failure lands \
         under the row for the same reason: after a failed create the next move is usually \
         to pick an existing workspace, so the list has to stay on screen. The remote's \
         message can be long; it clamps to three lines and carries the rest in a tooltip.",
    );
}

// ════════════════════════════════════════════════════════════════════════
/// 새 행 한 상태를 실제 pane 폭(440px)에서 보여주는 스트립 — 아래에 ws 행 하나를
/// 같이 깔아 두 행의 좌측 정렬선이 픽셀 동일한지 눈으로 확인할 수 있게 한다.
fn new_row_strip(ui: &mut egui::Ui, theme: &Theme, state: NewRow) {
    let peek = &WORKSPACES[0];
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_width(STRIP_W.value());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.vertical(|ui| {
                ui.set_width(STRIP_W.value());
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                new_ws_row(ui, theme, state);
                // 생성 중에는 아래 목록이 dim + inert 된다.
                let dim = if state.creating() { 0.5 } else { 1.0 };
                ui.scope(|ui| {
                    ui.set_opacity(dim);
                    ws_row(ui, theme, peek, false);
                });
            });
        });
}

/// 680×460 카드 한 장.
fn ra_card(ui: &mut egui::Ui, theme: &Theme, state: RaState) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_width(FRAME_W.value());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.vertical(|ui| {
                ui.set_width(FRAME_W.value());
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                header(ui, theme);
                body(ui, theme, state);
                footer(ui, theme, state);
            });
        });
}

fn header(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(FRAME_W.value(), HEADER_H), egui::Sense::hover());
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
        theme.icon_glyph_size_md,
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
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(FRAME_W.value(), BODY_H.value()),
        egui::Sense::hover(),
    );
    let left = egui::Rect::from_min_size(rect.min, egui::vec2(LEFT_W, BODY_H.value()));
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
    let hdr = egui::Rect::from_min_size(rect.min, egui::vec2(LEFT_W, CAPS_HEADER_H.value()));
    let (_, _) = col.allocate_exact_size(
        egui::vec2(LEFT_W, CAPS_HEADER_H.value()),
        egui::Sense::hover(),
    );
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
        RaState::Empty => "media-nas",
        _ => "prod-web",
    };
    for p in PROFILES {
        profile_row(&mut col, theme, p, p.name == sel_name);
    }
}

fn profile_row(ui: &mut egui::Ui, theme: &Theme, p: &Prof, selected: bool) {
    let w = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(w, PROFILE_ROW_H.value()), egui::Sense::hover());
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        selected_bar(ui, theme, rect);
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
    child.spacing_mut().item_spacing.y = STRUCT_GAP_2.value();
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
        // loaded / empty 는 같은 경로 — ws 목록의 길이만 다르다. empty 는 새 행을
        // 미리 선택해 두어 pane 이 뜬 순간부터 footer 가 살아 있다.
        RaState::Loaded => loaded_pane(
            ui,
            theme,
            rect,
            "prod-web",
            NewRow::Rest,
            WORKSPACES,
            Some("agents-prod"),
        ),
        RaState::Empty => loaded_pane(ui, theme, rect, "media-nas", NewRow::Selected, &[], None),
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
            "Establishing the SSH tunnel to gb10 and listing workspaces. If the host does not \
             respond, the lookup stops on its own after 20s.",
            false,
        ),
        // 실제 에러 클래스와 동기화(갤러리 완전성 정책) — `PortDiscoveryFailureKind::
        // RemoteInstanceNotRunning` (`crates/tasty-ssh/src/lib.rs`), 문구는
        // `lang/en.toml` `ssh.port_discovery.instance_not_running` 과 동일. 원격
        // stderr/포트 파일 경로 같은 내부 구현은 노출하지 않는다.
        RaState::Error => center_state(
            ui,
            theme,
            rect,
            icons::ALERT_TRIANGLE,
            theme.accent_danger().to_egui(),
            false,
            "Can't connect",
            theme.text_primary(),
            "No tasty instance appears to be running on the remote host.",
            true,
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
    col.add_space((rect.height() - EMPTY_BLOCK_H).max(0.0) * 0.5);
    col.spacing_mut().item_spacing.y = theme.spacing_sm.value();
    if spinner {
        Spinner::new().size(EMPTY_GLYPH).show(&mut col, theme);
    } else {
        kit::icon(&mut col, glyph, LogicalPx(EMPTY_GLYPH), glyph_color);
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

/// caps 헤더 + "+ New workspace" 행 + (ws 목록 | empty 한 줄).
///
/// 렌더 분기가 하나뿐이라 `ws` 가 비어도 이 경로를 그대로 탄다 — 목록이 비는 것은
/// "행이 하나인 목록"이지 다른 화면이 아니다.
fn loaded_pane(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    profile: &str,
    new_row: NewRow,
    ws: &[Ws],
    sel_ws: Option<&str>,
) {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    // caps 헤더 — "REMOTE WORKSPACES · {profile}". 생성이라는 사실은 행 라벨이 말하므로
    // 그룹을 설명하는 이 문구는 새 행이 생겨도 그대로다.
    let (hdr, _) = col.allocate_exact_size(
        egui::vec2(rect.width(), CAPS_HEADER_H.value()),
        egui::Sense::hover(),
    );
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
        format!("· {profile}"),
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    new_ws_row(&mut col, theme, new_row);
    if ws.is_empty() {
        empty_line(&mut col, theme, profile);
    } else {
        for w in ws {
            ws_row(&mut col, theme, w, sel_ws == Some(w.name));
        }
    }
}

/// 원격이 닿기는 하는데 ws 가 없을 때 새 행 아래 붙는 muted 한 줄. 이름 열은 위
/// 행들과 같은 정렬선에서 시작한다(선행 dot 슬롯 폭 스페이서).
fn empty_line(ui: &mut egui::Ui, theme: &Theme, profile: &str) {
    let width = ui.available_width();
    let h = theme.spacing_xs.value() * 2.0 + theme.font_size_caption.value() * theme.line_height_ui;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let x = rect.left()
        + theme.spacing_md.value()
        + theme.status_dot_size().value()
        + theme.spacing_sm.value();
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{profile} is reachable but has no workspaces yet."),
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
}

/// "+ New workspace" — loaded 목록의 첫 행. ws 행과 같은 34px 박스이고, 실제 원격
/// 워크스페이스와는 **세 채널 동시**로 구분된다(글리프 · accent 라벨 · 아래 구분선).
/// 색 하나로만 구분하지 않는다.
fn new_ws_row(ui: &mut egui::Ui, theme: &Theme, state: NewRow) {
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, WS_ROW_H.value()), egui::Sense::hover());
    if state.selected() {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        selected_bar(ui, theme, rect);
    } else if state == NewRow::Hover {
        ui.painter()
            .rect_filled(rect, 0.0, theme.overlay_hover().to_egui());
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
    let glyph_c = if state.creating() {
        theme.text_muted()
    } else if state.failed() {
        theme.accent_danger()
    } else {
        theme.accent_primary()
    };
    dot_slot_glyph(&mut child, theme, state, glyph_c.to_egui());
    // selected 에서만 accent 를 놓는다 — surface-active 위의 accent 는 3.17:1 이라
    // 고른 순간 가장 안 읽힌다. 구분은 글리프·구분선·accent 바가 계속 진다.
    let label_c = if state.creating() {
        theme.text_muted()
    } else if state.selected() {
        theme.text_primary()
    } else {
        theme.accent_primary()
    };
    child.label(
        egui::RichText::new(if state.creating() {
            "Creating workspace…"
        } else {
            "New workspace"
        })
        .size(theme.font_size_body.value())
        .strong()
        .color(label_c.to_egui()),
    );
    // 우측 슬롯 — status dot·pane 수·배지는 의미상 없는 행이라 캡션 하나뿐.
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if !state.creating() && !state.failed() {
            ui.label(
                egui::RichText::new("on remote")
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        }
    });
    if state.failed() {
        new_ws_error(ui, theme);
    }
    // 행 아래 1px 구분선 — 새 행 그룹을 닫는다.
    row_separator(ui, theme);
}

/// 이름 열 앞의 status-dot 슬롯(8px)을 할당한다. 목록의 **모든** 행이 이 한 함수로
/// 슬롯을 잡으므로 이름 열의 좌측 정렬선이 픽셀 동일해진다 — 새 행의 14px 글리프는
/// 슬롯보다 넓지만 좌우로 대칭 overflow 하므로 정렬선을 밀지 않는다.
fn dot_slot(ui: &mut egui::Ui, theme: &Theme) -> egui::Rect {
    let (slot, _) = ui.allocate_exact_size(
        egui::vec2(
            theme.status_dot_size().value(),
            theme.icon_glyph_size_sm.value(),
        ),
        egui::Sense::hover(),
    );
    slot
}

/// 새 행의 글리프 — 슬롯 중심에 놓인 14px `plus`(실패 시 `alertTriangle`,
/// 생성 중이면 Spinner).
fn dot_slot_glyph(ui: &mut egui::Ui, theme: &Theme, state: NewRow, color: egui::Color32) {
    let size = theme.icon_glyph_size_sm.value();
    let g = egui::Rect::from_center_size(dot_slot(ui, theme).center(), egui::vec2(size, size));
    if state.creating() {
        let mut c = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(g)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        Spinner::new().size(size).color(color).show(&mut c, theme);
    } else {
        let glyph = if state.failed() {
            icons::ALERT_TRIANGLE
        } else {
            icons::PLUS
        };
        glyph.image(size, color).paint_at(ui, g);
    }
}

/// ws 행의 실행 dot — 같은 슬롯 안에 그린다. `status_dot` 은 라벨이 비어도 dot 뒤에
/// 자기 gap 을 할당하므로 그대로 부르면 이름 열이 새 행보다 밀린다. 슬롯을 먼저
/// 잡고 그 안의 child 에 그려서, 위젯이 삼키는 여백이 정렬선에 새지 않게 한다.
fn dot_slot_status(ui: &mut egui::Ui, theme: &Theme, kind: StatusKind, pulse: bool) {
    let slot = dot_slot(ui, theme);
    let mut c = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(slot)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    status_dot(&mut c, theme, kind, "", pulse, false);
}

/// 생성 실패 — 행 하단 인라인. connect-error center-state 는 "목록 자체를 못 받은"
/// 경우의 어휘이고, 여기서는 목록을 이미 쥐고 있으므로 가리지 않는다.
fn new_ws_error(ui: &mut egui::Ui, theme: &Theme) {
    let width = ui.available_width();
    let cap_h = theme.font_size_caption.value() * theme.line_height_ui;
    let btn_h = ControlSize::Sm.height(theme);
    let h = theme.spacing_xs.value() * 2.0 + cap_h + btn_h + theme.spacing_sm.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left()
                + theme.spacing_md.value()
                + theme.status_dot_size().value()
                + theme.spacing_sm.value(),
            rect.top() + theme.spacing_xs.value(),
        ),
        egui::pos2(
            rect.right() - theme.spacing_md.value(),
            rect.bottom() - theme.spacing_sm.value(),
        ),
    );
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing.y = theme.spacing_xs.value();
    col.label(
        egui::RichText::new(CREATE_ERROR)
            .size(theme.font_size_caption.value())
            .color(theme.accent_danger().to_egui()),
    );
    Button::new("Try again")
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
        .show(&mut col, theme);
}

/// 행 아래 1px 구분선 + 위/아래 xs 마진.
fn row_separator(ui: &mut egui::Ui, theme: &Theme) {
    let width = ui.available_width();
    let m = theme.spacing_xs.value();
    let t = theme.border_width.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, m * 2.0 + t), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + m + t * 0.5,
        egui::Stroke::new(t, theme.separator.to_egui()),
    );
}

/// 선택 행의 inset accent 좌측바 — listctrl 과 같은 2px 토큰.
fn selected_bar(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let bar = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(theme.listctrl_selected_bar_width().value(), rect.height()),
    );
    ui.painter()
        .rect_filled(bar, 0.0, theme.listctrl_selected_bar().to_egui());
}

fn ws_row(ui: &mut egui::Ui, theme: &Theme, w: &Ws, selected: bool) {
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, WS_ROW_H.value()), egui::Sense::hover());
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, theme.surface_active().to_egui());
        selected_bar(ui, theme, rect);
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
    dot_slot_status(&mut child, theme, kind, w.busy);
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
        theme.font_size_caption,
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
    // 경고 글리프는 아이콘 스케일 xs(12), 글리프↔라벨 간격은 spacing_xs(4).
    let warn_glyph = theme.icon_glyph_size_xs.value();
    let warn_gap = theme.spacing_xs.value();
    let icon_w = if warn_icon {
        warn_glyph + warn_gap
    } else {
        0.0
    };
    let w = pad_x * 2.0 + icon_w + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, BADGE_H.value()), egui::Sense::hover());
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
            egui::pos2(tx, rect.center().y - warn_glyph * 0.5),
            egui::vec2(warn_glyph, warn_glyph),
        );
        icons::ALERT_TRIANGLE
            .image(warn_glyph, color)
            .paint_at(ui, ir);
        tx += warn_glyph + warn_gap;
    }
    ui.painter().galley(
        egui::pos2(tx, rect.center().y - galley.rect.height() * 0.5),
        galley,
        color,
    );
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: RaState) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(FRAME_W.value(), FOOTER_H), egui::Sense::hover());
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
    // Connect 는 목록이 떠 있고 행이 선택됐을 때만 활성. 새 행이 선택된 상태(empty 는
    // 그 행이 미리 선택돼 있다)에서는 버튼이 둘 중 무엇을 할지 말해야 한다.
    Button::new(if state == RaState::Empty {
        "Create & connect"
    } else {
        "Connect"
    })
    .variant(ButtonVariant::Primary)
    .enabled(matches!(state, RaState::Loaded | RaState::Empty))
    .show(&mut child, theme);
    // 조회 중에는 같은 ghost 버튼이 "조회 중단"이다 — 팝업을 닫지 않고 Connecting 을
    // 빠져나가는 수단(디자인 원본의 요소를 그대로 쓰되 문구만 상태에 맞춘다).
    Button::new(if state == RaState::Connecting {
        "Stop"
    } else {
        "Cancel"
    })
    .variant(ButtonVariant::Ghost)
    .show(&mut child, theme);
}
