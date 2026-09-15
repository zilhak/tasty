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

mod new_row;
mod panes;
mod rows;

pub use new_row::draw_new_row;

use panes::{left_pane, right_pane};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, IconButton, IconButtonVariant};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

// ── 프레임 고정 치수 (디자인 raw px — 화면 전용 고정값) ──
const FRAME_W: LogicalPx = LogicalPx(680.0);
const FRAME_H: LogicalPx = LogicalPx(460.0);
const LEFT_W: LogicalPx = LogicalPx(240.0);
const HEADER_H: LogicalPx = LogicalPx(47.0); // padding 10/10 + content 27
const HEADER_PAD_L: LogicalPx = LogicalPx(14.0); // 디자인 L14 (size-14)
const FOOTER_H: LogicalPx = LogicalPx(49.0);
const BODY_H: LogicalPx = FRAME_H.minus(HEADER_H).minus(FOOTER_H);
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
            ("shadow", "shadow-modal — occupies the viewport (centered)"),
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

/// 680×460 카드 한 장.
fn ra_card(ui: &mut egui::Ui, theme: &Theme, state: RaState) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        // 본체 `remote_attach` popup 은 anchored 명부에도 shadowless 명부에도 없어
        // 뷰포트를 점유하는 centered 표면으로 판정된다 = SCOPE RULE(ADR-0254)의 modal
        // 갈래(`popup/draw.rs::popup_shadow`). def 도 중앙 고정 · 이동/리사이즈 없음이다.
        // 이 specimen 은 공유 셸 키트를 안 쓰고 프레임을 직접 그리므로 갈래도 여기서
        // 직접 얹는다.
        .shadow(theme.shadow_modal().to_egui())
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
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(FRAME_W.value(), HEADER_H.value()),
        egui::Sense::hover(),
    );
    // borderBottom separator.
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    // 디자인 padding 위 10 · 오른쪽 10 · 아래 10 · 왼쪽 14.
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + HEADER_PAD_L.value(),
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
    let left = egui::Rect::from_min_size(rect.min, egui::vec2(LEFT_W.value(), BODY_H.value()));
    let right = egui::Rect::from_min_max(
        egui::pos2(rect.left() + LEFT_W.value(), rect.top()),
        rect.max,
    );
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

fn footer(ui: &mut egui::Ui, theme: &Theme, state: RaState) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(FRAME_W.value(), FOOTER_H.value()),
        egui::Sense::hover(),
    );
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
