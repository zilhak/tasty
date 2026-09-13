//! Settings › Keybindings › Import / Export — 디자인(4) Overlays `kbimportexport` Section.
//!
//! 전사 원본: `ui_kits/terminal/overlays/kb_import_export.jsx`(`KbImportExportSubtab` ·
//! `IeDiffTable` · `IeMigrateCard`/`IeMigrateRow` · `IeActionRow`) +
//! `ui_kits/terminal/overlays/settings_window.jsx`(`KB_L2_SEPARATED` · 창 자체 toast) +
//! `gallery/overlays-windows.jsx` 의 Spec 3 종(`IeL2Tail` · `IeEntry` · `IeGrid` ·
//! `IeMigrateG` · `IeBackBarG` · `IeNotices`).
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

use std::cell::RefCell;
use std::collections::BTreeSet;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, DrillDown, DrillDownView, TagVariant, checkbox, select, tag,
};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::toast_card::{self, CardColors, ToastKind};
use crate::catalog::widgets::dialog as kit;

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
/// 그룹 헤더의 chevron ↔ 그룹명 간격 — jsx `gap: 6`(그리드 밖 값, 스냅하지 않는다).
const GROUP_CHEVRON_GAP: LogicalPx = LogicalPx(6.0);
/// plugin 행 부제의 점 ↔ plugin 이름 간격 — jsx `gap: 5`.
const PLUGIN_DOT_GAP: LogicalPx = LogicalPx(5.0);
/// 마이그레이션 카드 채움 — jsx `color-mix(tone 11%)`.
const MIGRATE_CARD_FILL: f32 = 0.11;
/// 마이그레이션 카드 테두리 — jsx `color-mix(tone 36%)`.
const MIGRATE_CARD_BORDER: f32 = 0.36;
/// 파싱 실패 블록 채움 — jsx `color-mix(accent-danger 12%)`.
const FAILURE_FILL: f32 = 0.12;
/// 파싱 실패 블록 테두리 — jsx `color-mix(accent-danger 35%)`.
const FAILURE_BORDER: f32 = 0.35;

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

/// 축 modifier Select 의 선택지 — jsx `MODIFIER_COMBOS`(비-macOS 7 종) 앞에 "아직 안 고름"
/// sentinel(`IE_PICK`)을 둔다.
const MODIFIER_OPTIONS: &[&str] = &[
    "— pick a modifier —",
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
    pending_modifier: usize,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        changed_only: false,
        collapsed: BTreeSet::from(["scripts"]),
        deselected: BTreeSet::new(),
        l2_filter_active: false,
        pending_modifier: 0,
    });
}

// ── Spec 1: L2 배치 · 진입 화면 · 내보내기 피드백 ─────────────────────────────────

pub fn draw_entry(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xl.value();
            STATE.with(|s| {
                let st = &mut *s.borrow_mut();
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                    l2_tail(ui, theme, st.l2_filter_active);
                    checkbox(
                        ui,
                        theme,
                        &mut st.l2_filter_active,
                        "filter active (separator hidden)",
                        true,
                    );
                });
            });
            ui.vertical(|ui| {
                ui.set_width(ENTRY_W.value());
                ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                entry(ui, theme);
                kit::hsep(ui, theme);
                caption(
                    ui,
                    theme,
                    "export feedback — the settings window's own toast",
                );
                export_toast(ui, theme);
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("L2 items", "one — “Import / Export”"),
            ("position", "last, after Plugins"),
            ("separator", "1px above the row (new axis)"),
            ("filtering", "separator hidden while filtering"),
            ("entry", "2 action rows, not a ListCtrl"),
            ("weights", "Import primary · Export secondary"),
            (
                "export feedback",
                "the window's own toast, carrying the path",
            ),
            ("file picker", "popup on the window's PopupManager"),
        ],
        &[
            TokenChip::new(
                "separator",
                "L2 separator + row rules",
                theme.separator.to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "action row bed",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "border-default",
                "action row edge",
                theme.border_default().to_egui(),
            ),
            TokenChip::new(
                "settings-sidebar-width",
                "200 — L2 column",
                theme.bg_sidebar().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Why a popup for the file picker and not another drill-down step: the drill-down is \
         already spoken for by the preview (list ⇄ detail), and the same picker serves both \
         directions — Export needs a save target, Import an open target. The settings window \
         already runs a PopupManager for the shortcut-conflict confirm, so this adds a case, \
         not a mechanism.",
    );
    spec::dont(
        ui,
        theme,
        "Don't give export its own result screen. There is nothing to do after a write, so the \
         confirmation is a toast with the resolved path and the user stays where they were.",
    );
}

/// jsx `IeL2Tail` + `settings_window.jsx` L2 행 — Keybindings L2 의 끝 네 행 · separator ·
/// 선택된 Import / Export.
fn l2_tail(ui: &mut egui::Ui, theme: &Theme, filter_active: bool) {
    egui::Frame::new()
        .fill(theme.bg_sidebar().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_all(theme.spacing_sm))
        .show(ui, |ui| {
            ui.set_width((theme.settings_sidebar_width() - theme.spacing_sm.scaled(2.0)).value());
            ui.spacing_mut().item_spacing.y = 0.0;
            for label in ["Explorer", "Scripts", "Preset", "Plugins"] {
                l2_row(ui, theme, label, false);
            }
            if !filter_active {
                l2_separator(ui, theme);
            }
            l2_row(ui, theme, "Import / Export", true);
        });
}

/// L2 separator — jsx `height: border-width · background: separator · margin: space-sm space-sm`.
fn l2_separator(ui: &mut egui::Ui, theme: &Theme) {
    let m = theme.spacing_sm.value();
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value() + m * 2.0),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        (rect.left() + m)..=(rect.right() - m),
        rect.center().y,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

/// L2 한 행 — 본체 `sidebar_row` 와 같은 치수(padding space-xs/space-sm, radius-sm).
fn l2_row(ui: &mut egui::Ui, theme: &Theme, label: &str, active: bool) {
    let h = theme.font_size_body.value() + theme.spacing_xs.value() * 2.0;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    if active {
        ui.painter().rect_filled(
            rect,
            theme.corner_radius_sm.value(),
            theme.surface_active().to_egui(),
        );
    }
    let fg = if active {
        theme.text_primary()
    } else {
        theme.text_muted()
    };
    ui.painter().text(
        egui::pos2(rect.left() + theme.spacing_sm.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_body.value()),
        fg.to_egui(),
    );
}

/// jsx `KbImportExportSubtab` 의 list 위치 — 안내문 + 액션 행 둘(Export secondary ·
/// Import primary).
fn entry(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
    intro(
        ui,
        theme,
        theme.measure_md,
        "Move your whole keybinding configuration between machines. Importing never applies \
         straight away — you see what changes first.",
    );
    action_row(
        ui,
        theme,
        icons::DOWNLOAD,
        "Export",
        "Writes every binding — general, quick switch, script bindings and plugin overrides — \
         to one file.",
        "Export…",
        ButtonVariant::Secondary,
    );
    action_row(
        ui,
        theme,
        icons::FILE,
        "Import",
        "Reads a keybinding file and shows the changes against your current bindings before \
         anything is written.",
        "Import…",
        ButtonVariant::Primary,
    );
}

/// jsx `IeActionRow` — glyph · 제목(13 primary) + 설명(12 muted, measure-md) · trailing 버튼.
fn action_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: MockGlyph,
    title: &str,
    desc: &str,
    button: &str,
    variant: ButtonVariant,
) {
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, theme.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                glyph_at(
                    ui,
                    glyph,
                    theme.icon_glyph_size_md,
                    theme.text_muted().to_egui(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    Button::new(button)
                        .variant(variant)
                        .size(ControlSize::Sm)
                        .show(ui, theme);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y =
                            tasty_ui_widgets::tokens::STRUCT_GAP_2.value();
                        ui.label(
                            egui::RichText::new(title)
                                .size(theme.font_size_body.value())
                                .color(theme.text_primary().to_egui()),
                        );
                        ui.scope(|ui| {
                            ui.set_max_width(theme.measure_md.value());
                            ui.label(
                                egui::RichText::new(desc)
                                    .size(theme.font_size_term_sm.value())
                                    .color(theme.text_muted().to_egui()),
                            );
                        });
                    });
                });
            });
        });
}

/// 내보내기 완료 — 설정 창 자체 `ToastManager` 의 success 카드에 해석된 경로.
fn export_toast(ui: &mut egui::Ui, theme: &Theme) {
    let text = format!("Exported to ~/tasty/{IE_FILE}");
    let font = egui::FontId::proportional(theme.font_size_body.value());
    let chrome = toast_card::ACCENT_BAR_WIDTH + toast_card::PADDING_X * 2.0;
    let wrap = theme.toast_max_width.value() - chrome;
    let galley = ui.fonts(|f| f.layout(text, font, theme.text_primary().to_egui(), wrap));
    let w = galley.rect.width() + chrome;
    let h = galley.rect.height() + toast_card::PADDING_Y * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    toast_card::draw_card(
        ui.painter(),
        theme,
        rect,
        CardColors {
            bg: theme.surface_raised().to_egui(),
            border: theme.border_strong().to_egui(),
            accent: toast_card::accent_color(ToastKind::Success, theme),
            text: theme.text_primary().to_egui(),
        },
        galley,
    );
}

// ── Spec 2: 가져오기 미리보기 — 그룹 헤더 · 선택 열 · 긴 표 ───────────────────────

pub fn draw_preview(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        STATE.with(|s| {
            let st = &mut *s.borrow_mut();
            detail_frame(
                ui,
                theme,
                "kb_ie_preview",
                PREVIEW_H,
                0,
                st,
                |ui, theme, st| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                    let (changed, total) = counts();
                    intro(
                        ui,
                        theme,
                        theme.measure_lg,
                        &format!(
                            "{IE_FILE} — {changed} of {total} bindings change, {} selected. Apply \
                         writes the selected rows into the draft; nothing is saved until you \
                         press Save.",
                            selected_count(st)
                        ),
                    );
                    diff_table(ui, theme, st);
                },
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "columns",
                "32px select · 1.6fr action · 1fr current · 1fr imported",
            ),
            ("group header", "spans all columns, on surface-raised"),
            (
                "group content",
                "select-all · name (mono 10 caps) · counts · chevron",
            ),
            (
                "groups",
                "general · quick switch · scripts · plugin overrides",
            ),
            ("default view", "changed only"),
            ("toggle", "“Show all {n}” in the back bar"),
            ("quick switch", "1 row per axis (3), slot count as sub-line"),
            (
                "plugin row",
                "command name + agent-dot plugin name, two lines",
            ),
            ("changed cell", "accent-primary, colour only (no bold)"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "group header bed",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "changed imported value",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("accent-agent", "plugin dot", theme.accent_agent().to_egui()),
            TokenChip::new("separator", "cell rules", theme.separator.to_egui()),
        ],
    );
    spec::note(
        ui,
        theme,
        "Apply writes the draft, Save commits it — the same two-stage contract as Preset, and \
         the intro line says so. The two never share a row: Apply sits in the back bar, Save in \
         the window footer.",
    );
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

/// jsx `IeDiffTable` — 4 열 grid(select · action · current · imported), 그룹 헤더 축.
fn diff_table(ui: &mut egui::Ui, theme: &Theme, st: &mut State) {
    let w = ui.available_width();
    let rest = (w - SELECT_COL_W.value()).max(0.0);
    let action_w = rest * 1.6 / 3.6;
    let value_w = rest / 3.6;
    let x_off = [
        0.0,
        SELECT_COL_W.value(),
        SELECT_COL_W.value() + action_w,
        SELECT_COL_W.value() + action_w + value_w,
    ];
    let col_w = [SELECT_COL_W.value(), action_w, value_w, value_w];
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let hairline = egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui());

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;

        // ── 열 헤더 (padding: 0 space-md space-sm) ──
        let head_font = egui::FontId::monospace(theme.font_size_micro.value());
        let head_h = ui.fonts(|f| f.row_height(&head_font)) + pad_y;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, head_h), egui::Sense::hover());
        for (i, text) in ["", "ACTION", "CURRENT", "IMPORTED"].iter().enumerate() {
            ui.painter().text(
                egui::pos2(rect.left() + x_off[i] + pad_x, rect.top()),
                egui::Align2::LEFT_TOP,
                *text,
                head_font.clone(),
                theme.text_muted().to_egui(),
            );
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - theme.border_width.value() * 0.5,
            hairline,
        );

        for g in GROUPS {
            let rows: Vec<&Row> = g
                .rows
                .iter()
                .filter(|r| !st.changed_only || r.cur != r.next)
                .collect();
            let open = !st.collapsed.contains(g.id);
            group_header(ui, theme, w, g, &rows, open, st);
            if !open {
                continue;
            }
            for r in rows {
                let key = (g.id, r.action);
                let row_h = diff_row_height(ui, theme, r);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::hover());
                // 선택 열 — 체크박스 가운데.
                // `checkbox` 는 라벨이 비어도 박스 뒤 gap 을 차지하므로, 박스 중심이 열 중심에
                // 오도록 시작점을 잡는다.
                let box_sz = theme.checkbox_size().value();
                let sel_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + (col_w[0] - box_sz) * 0.5, rect.top()),
                    egui::vec2(col_w[0], row_h),
                );
                let mut on = !st.deselected.contains(&key);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(sel_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        if checkbox(ui, theme, &mut on, "", true).changed() {
                            if on {
                                st.deselected.remove(&key);
                            } else {
                                st.deselected.insert(key);
                            }
                        }
                    },
                );
                // Action — body text-secondary + 부제(plugin 점 · 슬롯 수).
                action_cell(ui, theme, rect, x_off[1] + pad_x, col_w[1] - pad_x * 2.0, r);
                // Current — mono muted.
                let mono = egui::FontId::monospace(theme.font_size_term_sm.value());
                value_cell(
                    ui,
                    rect,
                    x_off[2] + pad_x,
                    col_w[2] - pad_x * 2.0,
                    r.cur,
                    mono.clone(),
                    theme.text_muted().to_egui(),
                );
                // Imported — blocked warning / changed accent-primary / 동일 muted.
                let fg = if r.blocked {
                    theme.accent_warning()
                } else if r.cur != r.next {
                    theme.accent_primary()
                } else {
                    theme.text_muted()
                };
                value_cell(
                    ui,
                    rect,
                    x_off[3] + pad_x,
                    col_w[3] - pad_x * 2.0,
                    r.next,
                    mono,
                    fg.to_egui(),
                );
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - theme.border_width.value() * 0.5,
                    hairline,
                );
            }
        }
    });
}

/// 그룹 헤더 — 네 열을 가로지르는 한 행(surface-raised). select-all · chevron · 그룹명 ·
/// `N changed · M total`. 열 헤더를 반복하지 않는다.
fn group_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    w: f32,
    g: &Group,
    shown: &[&Row],
    open: bool,
    st: &mut State,
) {
    let h = theme
        .checkbox_size()
        .value()
        .max(theme.font_size_micro.value())
        + theme.spacing_sm.value() * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.surface_raised().to_egui());
    let inner = rect.shrink2(egui::vec2(theme.spacing_md.value(), 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            let mut all = !shown.is_empty()
                && shown
                    .iter()
                    .all(|r| !st.deselected.contains(&(g.id, r.action)));
            if checkbox(ui, theme, &mut all, "", true).changed() {
                for r in g.rows {
                    if all {
                        st.deselected.remove(&(g.id, r.action));
                    } else {
                        st.deselected.insert((g.id, r.action));
                    }
                }
            }
            // chevron + 그룹명 — 한 버튼(접힘 토글).
            let glyph = theme.icon_glyph_size_sm.value();
            let galley = ui.painter().layout_no_wrap(
                g.label.to_uppercase(),
                egui::FontId::monospace(theme.font_size_micro.value()),
                theme.text_secondary().to_egui(),
            );
            let bw = glyph + GROUP_CHEVRON_GAP.value() + galley.rect.width();
            let (br, resp) = ui.allocate_exact_size(egui::vec2(bw, h), egui::Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let chevron = if open {
                icons::CHEVRON_DOWN
            } else {
                icons::CHEVRON_RIGHT
            };
            let cr = egui::Rect::from_min_size(
                egui::pos2(br.left(), br.center().y - glyph * 0.5),
                egui::vec2(glyph, glyph),
            );
            chevron
                .image(glyph, theme.text_muted().to_egui())
                .paint_at(ui, cr);
            ui.painter().galley(
                egui::pos2(
                    br.left() + glyph + GROUP_CHEVRON_GAP.value(),
                    br.center().y - galley.rect.height() * 0.5,
                ),
                galley,
                egui::Color32::PLACEHOLDER,
            );
            if resp.clicked() {
                if open {
                    st.collapsed.insert(g.id);
                } else {
                    st.collapsed.remove(g.id);
                }
            }
            let changed = g.rows.iter().filter(|r| r.cur != r.next).count();
            ui.label(
                egui::RichText::new(format!("{changed} changed · {} total", g.rows.len()))
                    .monospace()
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        },
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

fn diff_row_height(ui: &egui::Ui, theme: &Theme, r: &Row) -> f32 {
    let body =
        ui.fonts(|f| f.row_height(&egui::FontId::proportional(theme.font_size_body.value())));
    let sub = if r.plugin.is_some() || r.note.is_some() {
        ui.fonts(|f| f.row_height(&egui::FontId::proportional(theme.font_size_micro.value())))
            + tasty_ui_widgets::tokens::STRUCT_GAP_2.value()
    } else {
        0.0
    };
    body + sub + theme.spacing_sm.value() * 2.0
}

fn action_cell(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, x: f32, max_w: f32, r: &Row) {
    let body_font = egui::FontId::proportional(theme.font_size_body.value());
    let title = truncated(
        ui,
        r.action,
        body_font,
        theme.text_secondary().to_egui(),
        max_w,
    );
    let has_sub = r.plugin.is_some() || r.note.is_some();
    let sub_h = if has_sub {
        ui.fonts(|f| f.row_height(&egui::FontId::proportional(theme.font_size_micro.value())))
            + tasty_ui_widgets::tokens::STRUCT_GAP_2.value()
    } else {
        0.0
    };
    let top = rect.center().y - (title.rect.height() + sub_h) * 0.5;
    let title_h = title.rect.height();
    ui.painter().galley(
        egui::pos2(rect.left() + x, top),
        title,
        egui::Color32::PLACEHOLDER,
    );
    let sub_y = top + title_h + tasty_ui_widgets::tokens::STRUCT_GAP_2.value();
    if let Some(plugin) = r.plugin {
        let d = theme.status_dot_size.value();
        let micro = egui::FontId::monospace(theme.font_size_micro.value());
        let g = truncated(ui, plugin, micro, theme.text_muted().to_egui(), max_w - d);
        let cy = sub_y + g.rect.height() * 0.5;
        ui.painter().circle_filled(
            egui::pos2(rect.left() + x + d * 0.5, cy),
            d * 0.5,
            theme.accent_agent().to_egui(),
        );
        ui.painter().galley(
            egui::pos2(rect.left() + x + d + PLUGIN_DOT_GAP.value(), sub_y),
            g,
            egui::Color32::PLACEHOLDER,
        );
    } else if let Some(note) = r.note {
        let g = truncated(
            ui,
            note,
            egui::FontId::proportional(theme.font_size_micro.value()),
            theme.text_muted().to_egui(),
            max_w,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + x, sub_y),
            g,
            egui::Color32::PLACEHOLDER,
        );
    }
}

fn value_cell(
    ui: &egui::Ui,
    rect: egui::Rect,
    x: f32,
    max_w: f32,
    text: &str,
    font: egui::FontId,
    fg: egui::Color32,
) {
    let g = truncated(ui, text, font, fg, max_w);
    let pos = egui::pos2(rect.left() + x, rect.center().y - g.rect.height() * 0.5);
    ui.painter().galley(pos, g, egui::Color32::PLACEHOLDER);
}

// ── Spec 3: Option 마이그레이션 — 미완료 · 완료 · 충돌 · unbound · 불필요 · 실패 ─────────

pub fn draw_migration(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
            STATE.with(|s| {
                let st = &mut *s.borrow_mut();
                // back bar(미해결 2) 아래 미완료 카드 — Apply 비활성.
                detail_frame(
                    ui,
                    theme,
                    "kb_ie_migration",
                    MIGRATION_H,
                    2,
                    st,
                    |ui, theme, st| {
                        migrate_card(ui, theme, false, st);
                    },
                );
            });
            caption(ui, theme, "resolved — Apply enabled");
            ui.scope(|ui| {
                ui.set_width(SPECIMEN_W.value());
                STATE.with(|s| migrate_card(ui, theme, true, &mut s.borrow_mut()));
            });
            caption(
                ui,
                theme,
                "notices — dropped plugin overrides · no migration needed · unreadable file",
            );
            ui.scope(|ui| {
                ui.set_width(SPECIMEN_W.value());
                notices(ui, theme);
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("position", "above the diff table; notice between"),
            ("gate", "Apply disabled while any row is unresolved"),
            (
                "counter",
                "“{n} of {m} unresolved” in the card header + back bar",
            ),
            ("widget A", "record slot — min 140 × 24, mono"),
            ("widget B", "modifier Select — 7 combos (non-macOS)"),
            ("label column", "288px — ja longest label measures 255px"),
            ("axis fan-out", "sub-line: “10 slots change with it”"),
            ("unbound", "counts as resolved, shown as a Tag"),
            ("not needed", "card absent + one intro sentence"),
            ("failure", "inline block in the detail area"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "pending card + “Not set”",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new(
                "accent-success",
                "resolved card + set check",
                theme.accent_success().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "conflict border + parse failure",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "record slot bed",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "text-disabled",
                "empty slot label",
                theme.text_disabled().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Two disabled Applies, two reasons. Preset shows a disabled button relabelled Applied \
         (nothing left to do). Here the label stays Apply and the reason is carried next to it \
         as “{n} unresolved” plus the card counter — a disabled button whose cause is off-screen \
         is a dead end, and relabelling would claim the import already happened.",
    );
    spec::dont(
        ui,
        theme,
        "Don't make “dropped plugin overrides” a warning callout. Nothing is wrong and there is \
         no action — a warning triangle on an unactionable fact trains people to ignore \
         triangles. It is one muted info line naming the plugins.",
    );
}

/// jsx `IeMigrateCard` — 톤 틴트 카드(헤더 · 설명 · 행들).
fn migrate_card(ui: &mut egui::Ui, theme: &Theme, done: bool, st: &mut State) {
    let rows: &[MigrateRow] = if done {
        &[
            MigrateRow {
                action: "Screenshot to clipboard",
                from: "Option+Shift+4",
                widget: Widget::Record,
                value: "Ctrl+Shift+4",
                state: MigrateState::Set,
                conflict: None,
                fanout: None,
            },
            MigrateRow {
                action: "Category axis modifier",
                from: "Option",
                widget: Widget::Modifier,
                value: "Ctrl+Alt",
                state: MigrateState::Set,
                conflict: None,
                fanout: Some("10 slots on this axis change with it"),
            },
            MigrateRow {
                action: "Jump to error",
                from: "Option+E",
                widget: Widget::Record,
                value: "Unbound",
                state: MigrateState::Unbound,
                conflict: None,
                fanout: None,
            },
        ]
    } else {
        &[
            MigrateRow {
                action: "Screenshot to clipboard",
                from: "Option+Shift+4",
                widget: Widget::Record,
                value: "Ctrl+Shift+4",
                state: MigrateState::Set,
                conflict: None,
                fanout: None,
            },
            MigrateRow {
                action: "Category axis modifier",
                from: "Option",
                widget: Widget::Modifier,
                value: "",
                state: MigrateState::Unset,
                conflict: None,
                fanout: Some("10 slots on this axis change with it"),
            },
            MigrateRow {
                action: "Toggle vi mode",
                from: "Option+V",
                widget: Widget::Record,
                value: "Ctrl+Shift+C",
                state: MigrateState::Conflict,
                conflict: Some("Also bound to Copy"),
                fanout: None,
            },
            MigrateRow {
                action: "Jump to error",
                from: "Option+E",
                widget: Widget::Record,
                value: "Not set",
                state: MigrateState::Unset,
                conflict: None,
                fanout: None,
            },
        ]
    };
    let tone = if done {
        theme.accent_success().to_egui()
    } else {
        theme.accent_warning().to_egui()
    };
    let total = if done { 4 } else { rows.len() };
    let left = rows
        .iter()
        .filter(|r| r.state == MigrateState::Unset)
        .count();
    egui::Frame::new()
        .fill(tone.gamma_multiply(MIGRATE_CARD_FILL))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            tone.gamma_multiply(MIGRATE_CARD_BORDER),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, theme.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                let glyph = if done {
                    icons::CHECK
                } else {
                    icons::ALERT_TRIANGLE
                };
                glyph_at(ui, glyph, theme.icon_glyph_size_md, tone);
                ui.label(
                    egui::RichText::new(if done {
                        "Option bindings resolved"
                    } else {
                        "Option bindings need a replacement"
                    })
                    .size(theme.font_size_body.value())
                    .color(tone),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let counter = if done {
                        format!("{total} of {total} resolved")
                    } else {
                        format!("{left} of {total} unresolved")
                    };
                    ui.label(
                        egui::RichText::new(counter)
                            .monospace()
                            .size(theme.font_size_caption.value())
                            .color(tone),
                    );
                });
            });
            intro_secondary(
                ui,
                theme,
                if done {
                    "Every option-bearing binding now has a replacement or is left unbound. Apply \
                     is enabled."
                } else {
                    "option never matches on this OS — these bindings would look bound and do \
                     nothing. Give each one a replacement, or leave it unbound. Apply stays \
                     disabled until none are left."
                },
            );
            tasty_ui_widgets::vspace(ui, theme.spacing_xs);
            for (i, r) in rows.iter().enumerate() {
                migrate_row(ui, theme, r, done, i, st);
            }
        });
}

/// jsx `IeMigrateRow` — 라벨 288 · 원래 조합 120 · → · 위젯 · trailing(check / Not set +
/// Leave unbound / Unbound Tag) + 부제(충돌 사유 또는 fan-out).
fn migrate_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    r: &MigrateRow,
    done: bool,
    index: usize,
    st: &mut State,
) {
    // jsx `padding: space-sm 0 · borderTop separator · gap 4` — 간격을 명시로만 준다.
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        // 행 상단 separator.
        let w = ui.available_width();
        let (sep, _) = ui.allocate_exact_size(
            egui::vec2(w, theme.border_width.value()),
            egui::Sense::hover(),
        );
        ui.painter().hline(
            sep.x_range(),
            sep.center().y,
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );
        tasty_ui_widgets::vspace(ui, theme.spacing_sm);
        ui.horizontal(|ui| {
            ui.set_min_height(theme.item_height_interactive.value());
            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
            fixed_label(
                ui,
                MIGRATE_LABEL_W,
                r.action,
                egui::FontId::proportional(theme.font_size_body.value()),
                theme.text_secondary().to_egui(),
            );
            fixed_label(
                ui,
                MIGRATE_FROM_W,
                r.from,
                egui::FontId::monospace(theme.font_size_term_sm.value()),
                theme.text_muted().to_egui(),
            );
            glyph_at(
                ui,
                icons::CHEVRON_RIGHT,
                theme.icon_glyph_size_sm,
                theme.text_muted().to_egui(),
            );
            match r.widget {
                Widget::Modifier => {
                    if done {
                        let mut idx = MODIFIER_OPTIONS
                            .iter()
                            .position(|o| *o == r.value)
                            .unwrap_or(0);
                        select(
                            ui,
                            theme,
                            &format!("kb_ie_modifier_done_{index}"),
                            &mut idx,
                            MODIFIER_OPTIONS,
                            theme.field_width_md.value(),
                            true,
                        );
                    } else {
                        select(
                            ui,
                            theme,
                            &format!("kb_ie_modifier_{index}"),
                            &mut st.pending_modifier,
                            MODIFIER_OPTIONS,
                            theme.field_width_md.value(),
                            true,
                        );
                    }
                }
                Widget::Record => record_slot(ui, theme, r),
            }
            match r.state {
                MigrateState::Set => glyph_at(
                    ui,
                    icons::CHECK,
                    theme.icon_glyph_size_sm,
                    theme.accent_success().to_egui(),
                ),
                MigrateState::Unbound => {
                    tag(
                        ui,
                        theme,
                        "Unbound — counts as resolved",
                        TagVariant::Default,
                        false,
                    );
                }
                MigrateState::Unset => {
                    ui.label(
                        egui::RichText::new("Not set")
                            .size(theme.font_size_caption.value())
                            .color(theme.accent_warning().to_egui()),
                    );
                    // 축 modifier 는 비울 수 없다 — "Leave unbound" 는 콤보 자리에만.
                    if matches!(r.widget, Widget::Record) {
                        Button::new("Leave unbound")
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, theme);
                    }
                }
                MigrateState::Conflict => {}
            }
        });
        if let Some(conflict) = r.conflict {
            tasty_ui_widgets::vspace(ui, theme.spacing_xs);
            ui.horizontal(|ui| {
                ui.add_space(MIGRATE_LABEL_W.value());
                ui.spacing_mut().item_spacing.x = GROUP_CHEVRON_GAP.value();
                glyph_at(
                    ui,
                    icons::ALERT_TRIANGLE,
                    theme.icon_glyph_size_sm,
                    theme.accent_danger().to_egui(),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{conflict} — the shortcut-conflict popup opens on Apply."
                    ))
                    .size(theme.font_size_caption.value())
                    .color(theme.accent_danger().to_egui()),
                );
            });
        } else if let Some(fanout) = r.fanout {
            tasty_ui_widgets::vspace(ui, theme.spacing_xs);
            ui.horizontal(|ui| {
                ui.add_space(MIGRATE_LABEL_W.value());
                ui.label(
                    egui::RichText::new(fanout)
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            });
        }
        tasty_ui_widgets::vspace(ui, theme.spacing_sm);
    });
}

/// 녹화 슬롯 — min 140 × 24 · mono · surface-raised · 충돌이면 danger 테두리 · 빈 값은
/// text-disabled.
fn record_slot(ui: &mut egui::Ui, theme: &Theme, r: &MigrateRow) {
    let font = egui::FontId::monospace(theme.font_size_term_sm.value());
    let empty = r.state == MigrateState::Unset;
    let fg = if empty {
        theme.text_disabled()
    } else {
        theme.text_primary()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(r.value.to_string(), font, fg.to_egui());
    let pad = theme.spacing_sm.value();
    let w = (galley.rect.width() + pad * 2.0).max(RECORD_SLOT_MIN_W.value());
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(w, RECORD_SLOT_H.value()), egui::Sense::click());
    let border = if r.state == MigrateState::Conflict {
        theme.accent_danger()
    } else {
        theme.border_default()
    };
    ui.painter().rect(
        rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), border.to_egui()),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.left() + pad,
            rect.center().y - galley.rect.height() * 0.5,
        ),
        galley,
        egui::Color32::PLACEHOLDER,
    );
}

/// jsx `IeNotices` — 버린 plugin override 안내(정보, 경고 아님) · 마이그레이션 불필요 안내문 ·
/// 파싱 실패 인라인 블록.
fn notices(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
    dropped_notice(ui, theme);
    intro(
        ui,
        theme,
        theme.measure_lg,
        &format!("{IE_FILE} — 7 of 73 bindings change. No option bindings to migrate."),
    );
    parse_failure(ui, theme);
}

fn dropped_notice(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        glyph_at(
            ui,
            icons::HELP_CIRCLE,
            theme.icon_glyph_size_sm,
            theme.text_muted().to_egui(),
        );
        ui.label(
            egui::RichText::new(format!(
                "2 plugin overrides were dropped — those plugins aren't installed here \
                 ({IE_DISCARDED})."
            ))
            .size(theme.font_size_term_sm.value())
            .color(theme.text_muted().to_egui()),
        );
    });
}

/// 파싱 실패 — 보던 대상에 대한 사실이라 toast/popup 이 아니라 detail 영역 안에 인라인.
fn parse_failure(ui: &mut egui::Ui, theme: &Theme) {
    let danger = theme.accent_danger().to_egui();
    egui::Frame::new()
        .fill(danger.gamma_multiply(FAILURE_FILL))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            danger.gamma_multiply(FAILURE_BORDER),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, theme.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                glyph_at(ui, icons::ALERT_CIRCLE, theme.icon_glyph_size_md, danger);
                ui.label(
                    egui::RichText::new("This file can't be read as keybindings")
                        .size(theme.font_size_body.value())
                        .color(danger),
                );
            });
            intro_secondary(
                ui,
                theme,
                "~/Downloads/settings.json — expected a keybinding export (TOML, a [keybindings] \
                 table); parsing stopped at line 1. Nothing was changed.",
            );
            tasty_ui_widgets::vspace(ui, theme.spacing_xs);
            Button::new("Choose another file")
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .show(ui, theme);
        });
}

// ── 헬퍼 ─────────────────────────────────────────────────────────────────────────

/// 안내문 — 12 muted, line-height ui, max-width `measure`.
fn intro(ui: &mut egui::Ui, theme: &Theme, measure: LogicalPx, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(measure.value());
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_term_sm.value())
                .color(theme.text_muted().to_egui()),
        );
    });
}

/// 카드 설명문 — 12 text-secondary, max-width measure-lg.
fn intro_secondary(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(theme.measure_lg.value());
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_term_sm.value())
                .color(theme.text_secondary().to_egui()),
        );
    });
}

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

fn glyph_at(ui: &mut egui::Ui, glyph: MockGlyph, size: LogicalPx, tint: egui::Color32) {
    let s = size.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
    glyph.image(s, tint).paint_at(ui, rect);
}

/// 고정 폭 한 줄 라벨(말줄임).
fn fixed_label(
    ui: &mut egui::Ui,
    width: LogicalPx,
    text: &str,
    font: egui::FontId,
    fg: egui::Color32,
) {
    let g = truncated(ui, text, font, fg, width.value());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width.value(), g.rect.height()),
        egui::Sense::hover(),
    );
    ui.painter().galley(rect.min, g, egui::Color32::PLACEHOLDER);
}

fn truncated(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    ui.fonts(|f| f.layout_job(job))
}
