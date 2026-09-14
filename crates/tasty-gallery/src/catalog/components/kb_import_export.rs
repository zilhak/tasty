//! Settings › Keybindings › Import / Export — 디자인(4) Overlays `kbimportexport` Section.
//!
//! 전사 원본: `ui_kits/terminal/overlays/kb_import_export.jsx`(`KbImportExportSubtab` ·
//! `IeDiffTable` · `IeMigrateCard`/`IeMigrateRow` · `IeActionRow`) +
//! `ui_kits/terminal/overlays/settings_window.jsx`(`KB_L2_SEPARATED` · 창 자체 toast) +
//! `gallery/overlays-windows.jsx` 의 Spec 4 종(`IeL2Tail` · `IeEntry` · `IeGrid` ·
//! `IeMigrateG` · `IeBackBarG` · `IeNotices` · `IeBlockG` · `IeExportFailG` · `IeBundleNoticesG` ·
//! `IeParseFailG` · `IeConflictSummaryG` · `IeModifierSelectG`).
//!
//! 본체 자리: `src/view/settings/ui/keybindings_tab.rs` 의 서브탭. 갤러리는 본체 binary 에
//! 의존하지 않으므로 같은 위젯(`DrillDown` · `Button` · `checkbox` · `select` · `tag`)과
//! 같은 토큰으로 **미러**한다(`settings_handler` · `settings_remote_transfer` 전례).
//!
//! 디자인이 이 화면에서 **새로 정의한 축이 셋**이다 — 기존 컴포넌트를 흉내 내지 않고
//! 정본 정의대로 그린다.
//!
//! - **L2 separator** — L2 행 모델에 "위에 구분선" 축. 필터가 활성이면 숨는다.
//! - **표 그룹 헤더** — 네 열을 가로지르는 한 행: 전체 선택 체크 · 접힘 chevron ·
//!   그룹명(mono micro caps) · `N changed · M total`. 열 헤더를 반복하지 않는다.
//! - **표 선택 열** — 32px 선두 열. 행 단위 적용.
//!
//! 전사 노트 — egui 한계로 관례를 따른 자리: `letter-spacing-caps` 는 미지원이라 mono
//! micro uppercase 로, `fontWeight: 600` 은 색 강조로 둔다(`preset.rs` · `hook_handlers.rs`
//! 관례). `color-mix(in srgb, tone X%, transparent)` 는 `gamma_multiply` 알파 감쇠로 근사한다
//! (`warning_callout` 전례). 폰트 10/11/12/13 은 `font_size_micro`/`caption`/`term_sm`/`body`.

//!
//! 모듈 경계는 본체 `keybindings_tab/import_export/` 와 **같은 이름**으로 가른다 — 화면 단위 그리기는
//! `entry`(Spec 1) · `diff_table`(Spec 2) · `migrate`(Spec 3) · `notices` · `open_values`(Spec 4 —
//! 첫 시안이 비워 둔 값 여섯), 공용 칠하기 헬퍼는
//! `paint`. 이 파일은 데모 데이터 · 상호작용 상태 · 두 Spec 이 함께 쓰는 `detail_frame` 과 치수
//! 상수를 든다(치수 상수는 본체 짝과의 값 일치 가드가 이 경로에서 읽는다).

mod diff_table;
mod entry;
mod migrate;
mod notices;
mod open_values;
mod paint;

use std::cell::RefCell;
use std::collections::BTreeSet;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, DrillDown, DrillDownView};

pub use diff_table::draw_preview;
pub use entry::draw_entry;
pub use migrate::draw_migration;
pub use open_values::draw_open_values;

/// specimen 폭 — 본체 설정 창 콘텐츠 컬럼(1100 창 − L2 200 − 좌우 패딩 16×2). jsx gallery 의
/// `maxWidth: 620` 에서는 ui kit 의 288 · 120 라벨 열이 들어가지 않아 본체 폭으로 둔다.
const SPECIMEN_W: LogicalPx = LogicalPx(868.0);
/// 진입 화면 컬럼 폭 — jsx gallery `IeEntry` 의 `maxWidth: 620`(L2 꼬리와 한 줄에 놓인다).
const ENTRY_W: LogicalPx = LogicalPx(620.0);
/// 미리보기 DrillDown 높이 — back bar + 표 한 화면.
const PREVIEW_H: LogicalPx = LogicalPx(420.0);
/// 마이그레이션 DrillDown 높이 — back bar + 미완료 카드.
const MIGRATION_H: LogicalPx = LogicalPx(440.0);
// 아래 치수 중 32 · 288 · 120 · 24 · 14 는 `size-*` 스케일 위의 값이다. jsx 는 원시 스케일
// 토큰을 직접 부르는데 그 스케일은 토큰 크레이트 밖에 열려 있지 않고, 같은 뜻의 semantic
// 이름도 없다 — jsx 를 인용한 명명 상수로 둔다.
/// 표 선택 열 — jsx `gridTemplateColumns: "var(--tasty-size-32) …"`.
const SELECT_COL_W: LogicalPx = LogicalPx(32.0);
/// 마이그레이션 행 라벨 열 — jsx `--tasty-size-288`(ja 최장 액션 라벨 실측 255px). 본체
/// 단축키 탭의 `LABEL_COL_WIDTH` 와 같은 값이다.
const MIGRATE_LABEL_W: LogicalPx = LogicalPx(288.0);
/// 마이그레이션 행 원래 조합 열 — jsx `--tasty-size-120`.
const MIGRATE_FROM_W: LogicalPx = LogicalPx(120.0);
/// 녹화 슬롯 최소 폭 — jsx `minWidth: 140`.
const RECORD_SLOT_MIN_W: LogicalPx = LogicalPx(140.0);
/// 녹화 슬롯 높이 — jsx `--tasty-size-24`.
const RECORD_SLOT_H: LogicalPx = LogicalPx(24.0);
/// 액션 행 · 마이그레이션 카드 · 실패 블록의 가로 패딩 — jsx `--tasty-size-14`.
const CARD_PAD_X: LogicalPx = LogicalPx(14.0);
/// 그룹 헤더의 chevron ↔ 그룹명, 경고 줄 글머리 ↔ 문구 간격 — jsx `gap: 6`(그리드 밖 값,
/// 스냅하지 않는다).
const GROUP_CHEVRON_GAP: LogicalPx = LogicalPx(6.0);
/// plugin 행 부제의 점 ↔ plugin 이름 간격 — jsx `gap: 5`.
const PLUGIN_DOT_GAP: LogicalPx = LogicalPx(5.0);
/// 충돌 개수 줄이 서는 최소 충돌 수 — 하나일 때는 행의 인라인 이유가 혼자 싣는다.
const CONFLICT_SUMMARY_FROM: usize = 2;
/// 경고 블록이 접기 전에 보이는 줄 수.
const NOTICE_FOLD_AT: usize = 3;
/// 마이그레이션 카드 채움 — jsx `color-mix(tone 11%)`.
const MIGRATE_CARD_FILL: f32 = 0.11;
/// 마이그레이션 카드 테두리 — jsx `color-mix(tone 36%)`.
const MIGRATE_CARD_BORDER: f32 = 0.36;
/// 알림 블록(파싱 실패 · 내보내기 실패 · 번들 경고) 채움 — jsx `IeBlockG` `color-mix(tone 12%)`.
const NOTICE_BLOCK_FILL: f32 = 0.12;
/// 알림 블록 테두리 — jsx `IeBlockG` `color-mix(tone 35%)`.
const NOTICE_BLOCK_BORDER: f32 = 0.35;

// ── 데모 데이터 (jsx `IE_GROUPS` · `IE_MIGRATE` · `IE_DISCARDED` 미러) ─────────────

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

// ── 상호작용 상태 (정적 specimen 이지만 새 축 셋은 눌러 볼 수 있게) ────────────────

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
            // 콘텐츠 영역을 높이째 잡아야 DrillDown 의 내부 스크롤이 그 높이를 채운다 —
            // stage 안의 가용 높이는 콘텐츠만큼이라 그대로 두면 본문이 한 줄로 접힌다.
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
