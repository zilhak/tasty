//! 단축키 가져오기·내보내기의 진입 화면, 비교 표, 호환성 처리, 오류 안내 예제.
//! 본체 바이너리에 의존하지 않고 공용 위젯으로 화면을 재현한다.
//! em 단위 자간 토큰은 생성기가 지원하지 않아 적용하지 않으며,
//! 굵기 차이는 글자색으로, color-mix는 알파 조절로 근사한다.

mod diff_table;
mod entry;
mod migrate;
mod notices;
mod open_values;
mod paint;
mod remaining_values;

use std::cell::RefCell;
use std::collections::BTreeSet;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, DrillDown, DrillDownView};

pub use diff_table::draw_preview;
pub use entry::draw_entry;
pub use migrate::draw_migration;
pub use open_values::draw_open_values;
pub use remaining_values::draw_remaining_values;

/// specimen 폭 — 본체 설정 창 콘텐츠 컬럼(1100 창 − L2 200 − 좌우 패딩 16×2). jsx gallery 의
/// `maxWidth: 620` 에서는 ui kit 의 288 · 120 라벨 열이 들어가지 않아 본체 폭으로 둔다.
const SPECIMEN_W: LogicalPx = LogicalPx(868.0);
/// 진입 화면 컬럼 폭 — jsx gallery `IeEntry` 의 `maxWidth: 620`(L2 꼬리와 한 줄에 놓인다).
const ENTRY_W: LogicalPx = LogicalPx(620.0);
/// 미리보기 DrillDown 높이 — back bar + 표 한 화면.
const PREVIEW_H: LogicalPx = LogicalPx(420.0);
/// 마이그레이션 DrillDown 높이 — back bar + 미완료 카드.
const MIGRATION_H: LogicalPx = LogicalPx(440.0);
// 이 치수의 디자인 토큰 이름이 저장된 DTCG에 없어 명명 상수를 사용한다.
/// 표 선택 열 — 디자인 `--tasty-kb-ie-select-column-width`(→ `size-32`).
const SELECT_COL_W: LogicalPx = LogicalPx(32.0);
/// 마이그레이션 행 라벨 열 — 디자인 `--tasty-kb-ie-action-column-width`(→ `size-288`, ja 최장 액션
/// 라벨 실측 255px). 본체 단축키 탭의 `LABEL_COL_WIDTH` 와 같은 값이다.
const MIGRATE_LABEL_W: LogicalPx = LogicalPx(288.0);
/// 마이그레이션 행 원래 조합 열 — 디자인 `--tasty-kb-ie-from-column-width`(→ `size-120`).
const MIGRATE_FROM_W: LogicalPx = LogicalPx(120.0);
/// 녹화 슬롯 최소 폭 — 디자인 `--tasty-kb-ie-slot-min-width`(→ `size-140`).
const RECORD_SLOT_MIN_W: LogicalPx = LogicalPx(140.0);
/// 그룹 헤더의 chevron ↔ 그룹명, 경고 줄 글머리 ↔ 문구 간격 — jsx `gap: 6`(그리드 밖 값,
/// 스냅하지 않는다).
const GROUP_CHEVRON_GAP: LogicalPx = LogicalPx(6.0);
/// plugin 행 부제의 점 ↔ plugin 이름 간격 — jsx `gap: 5`.
const PLUGIN_DOT_GAP: LogicalPx = LogicalPx(5.0);
/// 충돌 개수 줄이 서는 최소 충돌 수 — 하나일 때는 행의 인라인 이유가 혼자 싣는다.
const CONFLICT_SUMMARY_FROM: usize = 2;
/// 경고 블록이 접기 전에 보이는 줄 수.
const NOTICE_FOLD_AT: usize = 3;

const IE_FILE: &str = "tasty-keybindings-2026-09-09.toml";
const IE_DISCARDED: &str = "k8s-lens, s3-browser";

struct Row {
    action: &'static str,
    cur: &'static str,
    next: &'static str,
    /// plugin override 행의 plugin 이름(agent 점 + mono 부제).
    plugin: Option<&'static str>,
    /// quick-switch 축 행의 슬롯 수 부제.
    note: Option<&'static str>,
    /// 마이그레이션이 끝나지 않아 imported 값이 정해지지 않은 행.
    blocked: bool,
}

const fn row(action: &'static str, cur: &'static str, next: &'static str) -> Row {
    Row {
        action,
        cur,
        next,
        plugin: None,
        note: None,
        blocked: false,
    }
}

struct Group {
    id: &'static str,
    label: &'static str,
    rows: &'static [Row],
}

const GROUPS: &[Group] = &[
    Group {
        id: "general",
        label: "General bindings",
        rows: &[
            row("Copy", "Ctrl+Shift+C", "Ctrl+Shift+C"),
            row("Paste", "Ctrl+Shift+V", "Ctrl+Shift+V"),
            row("Command palette", "Ctrl+K", "Ctrl+Shift+P"),
            row("Split vertical", "Ctrl+D", "Ctrl+Alt+D"),
            row("Screenshot to clipboard", "None", "Ctrl+Shift+4"),
            row("Find", "Ctrl+F", "Ctrl+F"),
        ],
    },
    Group {
        id: "quickswitch",
        label: "Quick switch (axis summary)",
        rows: &[
            Row {
                note: Some("10 slots follow this axis"),
                ..row("Tab axis", "Alt+1…0", "Ctrl+1…0")
            },
            Row {
                note: Some("9 slots"),
                ..row("Workspace axis", "Alt+Shift+1…9", "Alt+Shift+1…9")
            },
            Row {
                note: Some("10 slots · needs migration"),
                blocked: true,
                ..row("Category axis", "Option+1…0", "— unresolved —")
            },
        ],
    },
    Group {
        id: "scripts",
        label: "Script bindings",
        rows: &[
            row("deploy-staging.lua", "Ctrl+Alt+1", "Ctrl+Alt+1"),
            row("rotate-logs.lua", "None", "Ctrl+Alt+2"),
        ],
    },
    Group {
        id: "plugins",
        label: "Plugin overrides",
        rows: &[
            Row {
                plugin: Some("git-helper"),
                ..row("open panel", "Ctrl+Alt+G", "Ctrl+Alt+G")
            },
            Row {
                plugin: Some("ai-review"),
                ..row("review staged", "Ctrl+Alt+R", "Ctrl+Shift+R")
            },
        ],
    },
];

/// 축 modifier Select 의 "아직 안 고름" placeholder — jsx `IE_PICK`. 값이 아니라 UI 폰트 ·
/// `text_placeholder` 색으로 그리고, 고르면 목록에서 빠진다.
const IE_PICK: &str = "Select a modifier";

/// 축 modifier Select 의 선택지 — jsx `MODIFIER_COMBOS`(비-macOS 7 종).
const MODIFIER_OPTIONS: &[&str] = &[
    "Ctrl",
    "Alt",
    "Shift",
    "Ctrl+Alt",
    "Ctrl+Shift",
    "Alt+Shift",
    "Ctrl+Alt+Shift",
];

/// 마이그레이션 행 위젯 — 콤보는 **녹화**, 축 modifier 는 **선택**.
#[derive(Clone, Copy)]
enum Widget {
    Record,
    Modifier,
}

/// 마이그레이션 행의 상태 넷 — jsx `IeMigrateRow` 의 `state`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MigrateState {
    Unset,
    Set,
    Conflict,
    Unbound,
}

struct MigrateRow {
    action: &'static str,
    from: &'static str,
    widget: Widget,
    value: &'static str,
    state: MigrateState,
    /// 충돌 상대 — `Conflict` 일 때만.
    conflict: Option<&'static str>,
    /// 축 modifier 가 바뀌면 함께 바뀌는 슬롯 안내 — 충돌이 없을 때만 보인다.
    fanout: Option<&'static str>,
}

struct State {
    changed_only: bool,
    collapsed: BTreeSet<&'static str>,
    deselected: BTreeSet<(&'static str, &'static str)>,
    l2_filter_active: bool,
    /// 미완료 카드의 축 modifier — 처음엔 안 고른 상태라 placeholder 가 보인다.
    pending_modifier: Option<usize>,
    /// Spec 4 경고 블록의 접힌 줄을 펼쳤는가.
    notices_expanded: bool,
    /// Spec 4 내보내기 실패 블록이 떠 있는가 — Try again · Choose another location 이 닫는다.
    export_failed: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        changed_only: false,
        collapsed: BTreeSet::from(["scripts"]),
        deselected: BTreeSet::new(),
        l2_filter_active: false,
        pending_modifier: None,
        notices_expanded: false,
        export_failed: true,
    });
}

/// 미리보기 detail — 실제 `DrillDown`(Detail) + back bar actions(토글 · 미해결 수 · Apply).
fn detail_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    height: LogicalPx,
    unresolved: usize,
    st: &mut State,
    body: impl FnOnce(&mut egui::Ui, &Theme, &mut State),
) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_width(SPECIMEN_W.value());
            let toggle = std::cell::Cell::new(false);
            let (_, total) = counts();
            let changed_only = st.changed_only;
            let actions = |ui: &mut egui::Ui, th: &Theme| {
                let enabled = unresolved == 0;
                Button::new("Apply")
                    .variant(ButtonVariant::Primary)
                    .size(ControlSize::Sm)
                    .enabled(enabled)
                    .show(ui, th);
                if unresolved > 0 {
                    ui.label(
                        egui::RichText::new(format!("{unresolved} unresolved"))
                            .monospace()
                            .size(th.font_size_caption.value())
                            .color(th.accent_warning().to_egui()),
                    );
                }
                let label = if changed_only {
                    format!("Show all {total}")
                } else {
                    "Changed only".to_string()
                };
                if Button::new(&label)
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .show(ui, th)
                    .clicked()
                {
                    toggle.set(true);
                }
            };
            // DrillDown 본문이 한 줄로 줄어들지 않도록 사용할 높이를 먼저 확보한다.
            ui.allocate_ui_with_layout(
                egui::vec2(SPECIMEN_W.value(), height.value()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    DrillDown::new(id_salt)
                        .view(DrillDownView::Detail)
                        .title("Import keybindings")
                        .back_label("Back")
                        .width(SPECIMEN_W.value())
                        .height(height.value())
                        .show(
                            ui,
                            theme,
                            |_, _| {},
                            |ui, th| {
                                egui::Frame::new()
                                    .inner_margin(tasty_ui_widgets::margin_all(th.spacing_lg))
                                    .show(ui, |ui| body(ui, th, st));
                            },
                            Some(&actions),
                        );
                },
            );
            if toggle.get() {
                st.changed_only = !st.changed_only;
            }
        });
}

fn counts() -> (usize, usize) {
    let total = GROUPS.iter().map(|g| g.rows.len()).sum();
    let changed = GROUPS
        .iter()
        .flat_map(|g| g.rows)
        .filter(|r| r.cur != r.next)
        .count();
    (changed, total)
}

fn selected_count(st: &State) -> usize {
    GROUPS
        .iter()
        .flat_map(|g| g.rows.iter().map(move |r| (g.id, r.action)))
        .filter(|k| !st.deselected.contains(k))
        .count()
}
