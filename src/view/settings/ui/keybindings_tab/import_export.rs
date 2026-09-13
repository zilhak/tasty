//! Keybindings › Import / Export 서브탭 — 단축키 구성 전량을 파일 하나로 옮긴다.
//!
//! 디자인 `ui_kits/terminal/overlays/kb_import_export.jsx`(`KbImportExportSubtab` ·
//! `IeDiffTable` · `IeMigrateCard`/`IeMigrateRow` · `IeActionRow`) 전사. 갤러리 specimen 은
//! `crates/tasty-gallery/src/catalog/components/kb_import_export.rs` 이고 같은 치수·토큰을 쓴다.
//!
//! - **List view** — 안내문 + 액션 행 둘(Export secondary · Import primary). 파일 선택은
//!   설정 창 `PopupManager` 의 파일 선택 popup 이다(DrillDown 한 단계가 아니다 — 그 자리는
//!   미리보기가 쓴다).
//! - **Detail view** — 가져온 파일의 미리보기. back bar 우측에 "변경만/전체" 토글 · 미해결 수 ·
//!   Apply. 본문은 안내문 → option 마이그레이션 카드(있을 때) → 버린 plugin 안내(있을 때) →
//!   4 그룹 diff 표. 파일을 못 읽었으면 본문 대신 인라인 실패 블록.
//! - **Apply 는 draft 까지** — 호스트 단축키는 settings draft, plugin override 는
//!   `plugin_shortcuts_draft` 에 쓴다. footer Save 가 둘 다 커밋한다(Preset 과 같은 경계).
//!
//! 적용 규칙(행 단위 · 번들에 없는 plugin override 보존 · 비워 두기 · 충돌 판정 범위)의
//! 근거는 `docs/adr/XXXX-keybinding-import-applies-selected-rows-onto-the-draft.md`.

mod model;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use tasty_host_plugin::keybinding_bundle::option_migration::{
    BindingConflict, BindingSite, ConflictPolicy, MigrationError, ReplacementKind, TargetOs,
    introduced_conflicts, preview_resolution, resolve_migration, scan_option_bindings,
};
use tasty_host_plugin::keybinding_bundle::{
    BundleError, BundleWarning, DecodeEnv, DecodedBundle, PluginShortcutOverrides, decode, encode,
};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, DrillDown, DrillDownView, TagVariant, checkbox, select,
    tag, vspace,
};

use crate::adapters::ui::icons;
use crate::adapters::ui::input::shortcuts::modifier_hint::all_modifier_combos;
use crate::i18n::{t, t_args, t_fmt, t_fmt2};
use crate::plugin::registry_state::ShortcutOverride;
use crate::settings::{GeneralSettings, KeybindingSettings, ScriptRegistry, Settings, SwitchStep};
use crate::settings_ui::PluginShortcutSnapshot;

pub(crate) use model::PluginShortcutDraft;
use model::{
    Group, MigrationRow, MigrationValue, RowKey, apply_rows, merged_overrides, override_of,
    plan_of, row_changed, row_keys, unresolved_count,
};

use super::{FieldKind, KeyCapture, LABEL_COL_WIDTH, RecordingSlot};

/// 표 선택 열 — jsx `gridTemplateColumns: "var(--tasty-size-32) …"`.
const SELECT_COL_W: LogicalPx = LogicalPx(32.0);
/// 마이그레이션 행 원래 조합 열 — jsx `--tasty-size-120`.
const MIGRATE_FROM_W: LogicalPx = LogicalPx(120.0);
/// 녹화 슬롯 최소 폭 — jsx `minWidth: 140`.
const RECORD_SLOT_MIN_W: LogicalPx = LogicalPx(140.0);
/// 녹화 슬롯 높이 — jsx `--tasty-size-24`.
const RECORD_SLOT_H: LogicalPx = LogicalPx(24.0);
/// 액션 행 · 마이그레이션 카드 · 실패 블록의 가로 패딩 — jsx `--tasty-size-14`.
const CARD_PAD_X: LogicalPx = LogicalPx(14.0);
/// 그룹 헤더 chevron ↔ 그룹명, 충돌 부제 아이콘 ↔ 문구 간격 — jsx `gap: 6`.
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

/// 내보내기 파일 선택의 결과 키.
pub(crate) const EXPORT_CONSUMER: &str = "keybindings_export";
/// 가져오기 파일 선택의 결과 키.
pub(crate) const IMPORT_CONSUMER: &str = "keybindings_import";
/// 마이그레이션 녹화 슬롯이 `recording_field` 에 남기는 필드 id — 실제 필드 이름과 겹치지 않는다.
const RECORDING_FIELD: &str = "__keybindings_import_migration";

/// 모달을 열 때 host 가 주입하는 plugin 쪽 원본 — export 는 스냅샷이 아니라 이것에서 만든다.
#[derive(Debug, Default, Clone)]
pub struct PluginBundleContext {
    /// `PluginsConfig.keybindings` 전량(비활성·미등록 plugin 포함).
    pub overrides: PluginShortcutOverrides,
    /// 이 환경에 설치된 plugin id — 가져오기가 나머지를 버린다.
    pub installed_plugin_ids: Vec<String>,
    /// plugin id → 표시 이름.
    pub plugin_names: BTreeMap<String, String>,
}

/// 화면이 셸에 요청하는 파일 선택.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportExportRequest {
    Export,
    Import,
}

/// 읽어 들인 번들.
struct Preview {
    file_name: String,
    keybindings: KeybindingSettings,
    overrides: PluginShortcutOverrides,
    /// 이 환경에 없는 plugin 과 버린 override 수.
    dropped: Vec<(String, usize)>,
    migration: Vec<MigrationRow>,
}

/// 읽지 못한 파일.
struct Failure {
    path: String,
    line: Option<usize>,
}

/// 서브탭 상태 — `SettingsUiState` 가 소유한다.
pub struct ImportExportState {
    preview: Option<Preview>,
    failure: Option<Failure>,
    /// 디자인 기본값: 변경된 행만.
    changed_only: bool,
    collapsed: BTreeSet<Group>,
    deselected: BTreeSet<RowKey>,
    request: Option<ImportExportRequest>,
    toast: Option<String>,
    /// Apply 가 충돌을 만나 열어야 하는 확인 문구.
    conflict_prompt: Option<String>,
    /// 확인 popup 의 답. 다음 draw 가 draft 를 들고 소비한다.
    conflict_answer: Option<bool>,
}

impl Default for ImportExportState {
    fn default() -> Self {
        Self {
            preview: None,
            failure: None,
            changed_only: true,
            collapsed: BTreeSet::new(),
            deselected: BTreeSet::new(),
            request: None,
            toast: None,
            conflict_prompt: None,
            conflict_answer: None,
        }
    }
}

impl ImportExportState {
    pub(crate) fn take_request(&mut self) -> Option<ImportExportRequest> {
        self.request.take()
    }

    /// 설정 창 자체 토스트로 올릴 문구(1 회).
    pub(crate) fn take_toast(&mut self) -> Option<String> {
        self.toast.take()
    }

    pub(crate) fn conflict_prompt(&self) -> Option<&str> {
        self.conflict_prompt.as_deref()
    }

    /// 충돌 확인 popup 의 답(수락/거절)을 남긴다.
    pub(crate) fn answer_conflict(&mut self, accepted: bool) {
        self.conflict_answer = Some(accepted);
    }

    /// 현재 구성(호스트 draft + 원본 위에 얹은 plugin draft)을 `path` 에 쓴다.
    pub(crate) fn export_to(
        &mut self,
        path: &Path,
        keybindings: &KeybindingSettings,
        ctx: &PluginBundleContext,
        plugin_draft: &PluginShortcutDraft,
    ) {
        let overrides = merged_overrides(&ctx.overrides, plugin_draft);
        let written = encode(keybindings, &overrides)
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(path, text).map_err(|e| e.to_string()));
        match written {
            Ok(()) => {
                self.toast = Some(t_fmt(
                    "settings.keybindings.ie_exported_toast",
                    &path.display().to_string(),
                ));
            }
            // 실패 표시는 디자인이 정하지 않았다 — 자리를 비워 두고 로그만 남긴다.
            Err(e) => tracing::error!("keybinding export to {} failed: {e}", path.display()),
        }
    }

    /// `path` 를 읽어 미리보기를 세운다. 못 읽으면 실패 블록이 대신 선다.
    pub(crate) fn import_from(
        &mut self,
        path: &Path,
        settings: &Settings,
        ctx: &PluginBundleContext,
    ) {
        self.preview = None;
        self.failure = None;
        self.conflict_prompt = None;
        self.conflict_answer = None;
        let decoded = match read_bundle(path, settings, ctx) {
            Ok(decoded) => decoded,
            Err(line) => {
                self.failure = Some(Failure {
                    path: path.display().to_string(),
                    line,
                });
                return;
            }
        };
        let dropped = dropped_plugins(path, &decoded.warnings);
        let migration = scan_option_bindings(
            &decoded.keybindings,
            &decoded.plugin_keybindings,
            TargetOs::host(),
        )
        .into_iter()
        .map(|found| MigrationRow {
            site: found.site,
            from: found.current,
            value: MigrationValue::Unset,
        })
        .collect();
        self.preview = Some(Preview {
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            keybindings: decoded.keybindings,
            overrides: decoded.plugin_keybindings,
            dropped,
            migration,
        });
        self.changed_only = true;
        self.collapsed.clear();
        self.deselected.clear();
    }

    /// 해소를 확정하고 고른 행을 두 draft 에 쓴다. 충돌이면 확인 문구를 세우고 멈춘다.
    fn apply(
        &mut self,
        settings: &mut Settings,
        plugin_draft: &mut PluginShortcutDraft,
        policy: ConflictPolicy,
        labels_src: (&PluginShortcutSnapshot, &PluginBundleContext),
    ) {
        let Some(preview) = self.preview.as_ref() else {
            return;
        };
        let resolved = if preview.migration.is_empty() {
            Ok((preview.keybindings.clone(), preview.overrides.clone()))
        } else {
            resolve_migration(
                &preview.keybindings,
                &preview.overrides,
                &plan_of(&preview.migration),
                policy,
            )
        };
        match resolved {
            Ok((kb, ov)) => {
                let keys: Vec<RowKey> = row_keys(&settings.keybindings, &kb, &ov)
                    .into_iter()
                    .filter(|k| !self.deselected.contains(k))
                    .collect();
                apply_rows(&keys, &mut settings.keybindings, plugin_draft, &kb, &ov);
                self.toast = Some(t("settings.keybindings.ie_applied_toast").to_string());
            }
            Err(MigrationError::Conflicts(conflicts)) if policy == ConflictPolicy::Reject => {
                let labels = Labels {
                    general: &settings.general,
                    scripts: &settings.scripts,
                    snapshot: labels_src.0,
                    names: &labels_src.1.plugin_names,
                };
                let plan = plan_of(&preview.migration);
                let message = conflicts
                    .iter()
                    .map(|c| {
                        let other = if plan.contains_key(&c.a) { &c.b } else { &c.a };
                        t_fmt2(
                            "settings.keybindings.conflict_message",
                            &KeybindingSettings::format_display(&c.combo, labels.general),
                            &labels.site(other),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.conflict_prompt = Some(message);
            }
            Err(e) => tracing::warn!("keybinding import apply refused: {e}"),
        }
    }
}

/// 표시 문자열 출처 묶음.
struct Labels<'a> {
    general: &'a GeneralSettings,
    scripts: &'a ScriptRegistry,
    snapshot: &'a PluginShortcutSnapshot,
    names: &'a BTreeMap<String, String>,
}

/// 파일을 읽어 번들로 푼다. 실패하면 알 수 있는 경우 TOML 이 멈춘 줄 번호를 돌려준다.
fn read_bundle(
    path: &Path,
    settings: &Settings,
    ctx: &PluginBundleContext,
) -> Result<DecodedBundle, Option<usize>> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        tracing::warn!("keybinding import: read {} failed: {e}", path.display());
        None
    })?;
    let script_ids: Vec<String> = settings.scripts.iter().map(|s| s.id.clone()).collect();
    let env = DecodeEnv {
        installed_plugin_ids: &ctx.installed_plugin_ids,
        known_script_ids: Some(&script_ids),
    };
    decode(&text, &env).map_err(|e| {
        tracing::warn!("keybinding import: {} is not a bundle: {e}", path.display());
        match &e {
            BundleError::Toml(te) => te
                .span()
                .map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1),
            _ => None,
        }
    })
}

/// 버린 plugin 경고만 화면 몫으로 모은다. 나머지 경고는 로그로만 남는다.
fn dropped_plugins(path: &Path, warnings: &[BundleWarning]) -> Vec<(String, usize)> {
    let mut dropped = Vec::new();
    for w in warnings {
        match w {
            BundleWarning::DroppedUninstalledPlugin {
                plugin_id,
                commands,
            } => dropped.push((plugin_id.clone(), *commands)),
            // 이 경고들의 표시는 디자인이 정하지 않았다 — 자리를 비워 두고 로그만 남긴다.
            other => tracing::warn!("keybinding import {}: {other}", path.display()),
        }
    }
    dropped
}

fn trim_label(s: &str) -> String {
    s.trim_end_matches(':').trim().to_string()
}

impl Labels<'_> {
    fn script_name(&self, id: &str) -> String {
        self.scripts
            .get(id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn plugin_name(&self, id: &str) -> String {
        self.names
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }

    fn command_title(&self, plugin_id: &str, command_id: &str) -> String {
        self.snapshot
            .rows
            .iter()
            .find(|r| r.plugin_id == plugin_id && r.command_id == command_id)
            .map(|r| t(&r.title_i18n_key).to_string())
            .unwrap_or_else(|| command_id.to_string())
    }

    fn manifest_default(&self, plugin_id: &str, command_id: &str) -> Option<&str> {
        self.snapshot
            .rows
            .iter()
            .find(|r| r.plugin_id == plugin_id && r.command_id == command_id)
            .and_then(|r| r.manifest_default.as_deref())
    }

    /// 한 자리의 사람이 읽는 이름 — 충돌 상대 표시와 마이그레이션 행 라벨.
    fn site(&self, site: &BindingSite) -> String {
        match site {
            BindingSite::GeneralBinding { field_id, .. } => {
                trim_label(KeybindingSettings::label_key_for(field_id).map_or(*field_id, t))
            }
            BindingSite::AxisModifier { axis } => trim_label(t(axis.modifier_label_key())),
            BindingSite::AxisSlot { axis, index } => {
                let key = match axis {
                    crate::settings::SwitchAxis::Tab => {
                        "settings.keybindings.tab_switch_slot_label"
                    }
                    crate::settings::SwitchAxis::Workspace => {
                        "settings.keybindings.workspace_switch_slot_label"
                    }
                    crate::settings::SwitchAxis::Category => {
                        "settings.keybindings.category_switch_slot_label"
                    }
                };
                trim_label(&t_fmt(key, &(index + 1).to_string()))
            }
            BindingSite::AxisStep { axis, step } => {
                let key = match (axis, step) {
                    (crate::settings::SwitchAxis::Tab, SwitchStep::Next) => {
                        "settings.keybindings.tab_switch_next_label"
                    }
                    (crate::settings::SwitchAxis::Tab, SwitchStep::Prev) => {
                        "settings.keybindings.tab_switch_prev_label"
                    }
                    (crate::settings::SwitchAxis::Workspace, SwitchStep::Next) => {
                        "settings.keybindings.workspace_switch_next_label"
                    }
                    (crate::settings::SwitchAxis::Workspace, SwitchStep::Prev) => {
                        "settings.keybindings.workspace_switch_prev_label"
                    }
                    (crate::settings::SwitchAxis::Category, SwitchStep::Next) => {
                        "settings.keybindings.category_switch_next_label"
                    }
                    (crate::settings::SwitchAxis::Category, SwitchStep::Prev) => {
                        "settings.keybindings.category_switch_prev_label"
                    }
                };
                trim_label(t(key))
            }
            BindingSite::ScriptBinding { script_id } => self.script_name(script_id),
            BindingSite::PluginOverride {
                plugin_id,
                command_id,
                ..
            } => self.command_title(plugin_id, command_id),
        }
    }

    fn combo(&self, combo: &str) -> String {
        KeybindingSettings::format_display(combo, self.general)
    }

    fn bindings(&self, v: &[String]) -> String {
        if v.is_empty() {
            t("settings.keybindings.hint_none").to_string()
        } else {
            v.iter()
                .map(|b| self.combo(b))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    /// 축 한 행의 값 — 합성된 범위(`Alt+1…0`).
    fn axis_summary(&self, axis: crate::settings::SwitchAxis, kb: &KeybindingSettings) -> String {
        let first = axis.slot(kb, 0).unwrap_or_default();
        let last = axis.slot(kb, axis.slot_count() - 1).unwrap_or_default();
        if first.is_empty() && last.is_empty() {
            return t("settings.keybindings.hint_none").to_string();
        }
        if axis.is_individual(kb) {
            format!("{}…{}", self.combo(first), self.combo(last))
        } else {
            let modifier = axis.modifier(kb);
            format!(
                "{}…{}",
                self.combo(&format!("{modifier}+{first}")),
                self.combo(last)
            )
        }
    }

    fn plugin_value(
        &self,
        plugin_id: &str,
        command_id: &str,
        ov: Option<&ShortcutOverride>,
    ) -> String {
        match ov {
            Some(ShortcutOverride::Key { value }) => self.bindings(value),
            Some(ShortcutOverride::Inherit { source }) => format!("@{source}"),
            Some(ShortcutOverride::None) => t("settings.keybindings.hint_none").to_string(),
            None => match self.manifest_default(plugin_id, command_id) {
                Some(d) => self.combo(d),
                None => t("settings.keybindings.hint_none").to_string(),
            },
        }
    }
}

// ── 뷰 모델 ─────────────────────────────────────────────────────────────────────

enum Sub {
    Plugin(String),
    Note(String),
}

struct RowView {
    key: RowKey,
    action: String,
    sub: Option<Sub>,
    cur: String,
    next: String,
    changed: bool,
    blocked: bool,
}

struct GroupView {
    group: Group,
    rows: Vec<RowView>,
}

struct MigrationView {
    action: String,
    from: String,
    kind: ReplacementKind,
    conflict: Option<String>,
    fanout: Option<String>,
}

struct ViewModel {
    groups: Vec<GroupView>,
    total: usize,
    picked: usize,
    unresolved: usize,
    migration: Vec<MigrationView>,
    intro: String,
    dropped: Option<String>,
}

fn build_view_model(
    preview: &Preview,
    state: &ImportExportState,
    current: &KeybindingSettings,
    current_overrides: &PluginShortcutOverrides,
    labels: &Labels<'_>,
) -> ViewModel {
    let plan = plan_of(&preview.migration);
    let (imported, imported_ov) = if preview.migration.is_empty() {
        (preview.keybindings.clone(), preview.overrides.clone())
    } else {
        preview_resolution(&preview.keybindings, &preview.overrides, &plan)
    };
    let blocked: BTreeSet<RowKey> = preview
        .migration
        .iter()
        .filter(|r| r.value == MigrationValue::Unset)
        .map(|r| RowKey::of_site(&r.site))
        .collect();

    let mut groups: Vec<GroupView> = Group::ALL
        .into_iter()
        .map(|group| GroupView {
            group,
            rows: Vec::new(),
        })
        .collect();
    for key in row_keys(current, &imported, &imported_ov) {
        let changed = row_changed(&key, current, current_overrides, &imported, &imported_ov);
        let is_blocked = blocked.contains(&key);
        let (action, sub, cur, next) = match &key {
            RowKey::General(field) => (
                trim_label(KeybindingSettings::label_key_for(field).map_or(*field, t)),
                None,
                labels.bindings(current.get_bindings(field).unwrap_or(&[])),
                labels.bindings(imported.get_bindings(field).unwrap_or(&[])),
            ),
            RowKey::Axis(axis) => {
                let action_key = match axis {
                    crate::settings::SwitchAxis::Tab => "settings.keybindings.ie_axis_tab",
                    crate::settings::SwitchAxis::Workspace => {
                        "settings.keybindings.ie_axis_workspace"
                    }
                    crate::settings::SwitchAxis::Category => {
                        "settings.keybindings.ie_axis_category"
                    }
                };
                let note_key = if is_blocked {
                    "settings.keybindings.ie_axis_slots_blocked"
                } else {
                    "settings.keybindings.ie_axis_slots"
                };
                (
                    t(action_key).to_string(),
                    Some(Sub::Note(t_fmt(note_key, &axis.slot_count().to_string()))),
                    labels.axis_summary(*axis, current),
                    labels.axis_summary(*axis, &imported),
                )
            }
            RowKey::Script(id) => (
                labels.script_name(id),
                None,
                labels.bindings(
                    &current
                        .script_binding_combo(id)
                        .map(|c| vec![c.to_string()])
                        .unwrap_or_default(),
                ),
                labels.bindings(
                    &imported
                        .script_binding_combo(id)
                        .map(|c| vec![c.to_string()])
                        .unwrap_or_default(),
                ),
            ),
            RowKey::Plugin {
                plugin_id,
                command_id,
            } => (
                labels.command_title(plugin_id, command_id),
                Some(Sub::Plugin(labels.plugin_name(plugin_id))),
                labels.plugin_value(
                    plugin_id,
                    command_id,
                    override_of(current_overrides, plugin_id, command_id),
                ),
                labels.plugin_value(
                    plugin_id,
                    command_id,
                    override_of(&imported_ov, plugin_id, command_id),
                ),
            ),
        };
        let next = if is_blocked {
            t("settings.keybindings.ie_unresolved_value").to_string()
        } else {
            next
        };
        let group = key.group();
        if let Some(g) = groups.iter_mut().find(|g| g.group == group) {
            g.rows.push(RowView {
                key,
                action,
                sub,
                cur,
                next,
                changed: changed || is_blocked,
                blocked: is_blocked,
            });
        }
    }
    let total: usize = groups.iter().map(|g| g.rows.len()).sum();
    let changed = groups
        .iter()
        .flat_map(|g| &g.rows)
        .filter(|r| r.changed)
        .count();
    let picked = groups
        .iter()
        .flat_map(|g| &g.rows)
        .filter(|r| !state.deselected.contains(&r.key))
        .count();

    let conflicts: Vec<BindingConflict> = if preview.migration.is_empty() {
        Vec::new()
    } else {
        introduced_conflicts(&preview.keybindings, &preview.overrides, &plan)
    };
    let migration = preview
        .migration
        .iter()
        .map(|r| {
            let conflict = if matches!(r.value, MigrationValue::Set(_)) {
                conflicts
                    .iter()
                    .find_map(|c| {
                        if c.a == r.site {
                            Some(&c.b)
                        } else if c.b == r.site {
                            Some(&c.a)
                        } else {
                            None
                        }
                    })
                    .map(|other| {
                        t_fmt(
                            "settings.keybindings.ie_migrate_conflict",
                            &labels.site(other),
                        )
                    })
            } else {
                None
            };
            let fanout = match &r.site {
                BindingSite::AxisModifier { axis } => Some(t_fmt(
                    "settings.keybindings.ie_axis_fanout",
                    &axis.slot_count().to_string(),
                )),
                _ => None,
            };
            MigrationView {
                action: labels.site(&r.site),
                from: labels.combo(&r.from),
                kind: r.site.replacement_kind(),
                conflict,
                fanout,
            }
        })
        .collect();

    let mut intro = t_args(
        "settings.keybindings.ie_preview_intro",
        &[
            &preview.file_name,
            &changed.to_string(),
            &total.to_string(),
            &picked.to_string(),
        ],
    );
    if preview.migration.is_empty() {
        intro.push_str(t("settings.keybindings.ie_no_migration"));
    }
    let dropped = (!preview.dropped.is_empty()).then(|| {
        let count: usize = preview.dropped.iter().map(|(_, n)| n).sum();
        let list = preview
            .dropped
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        t_fmt2("settings.keybindings.ie_dropped", &count.to_string(), &list)
    });

    ViewModel {
        groups,
        total,
        picked,
        unresolved: unresolved_count(&preview.migration),
        migration,
        intro,
        dropped,
    }
}

// ── 그리기 ───────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_import_export_subtab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    state: &mut ImportExportState,
    ctx: &PluginBundleContext,
    snapshot: &PluginShortcutSnapshot,
    plugin_draft: &mut PluginShortcutDraft,
    recording_field: &mut Option<RecordingSlot>,
    captured: &KeyCapture,
) {
    let th = crate::theme::theme();

    // 마이그레이션 녹화 결과 — Escape 는 기존 규칙대로 "비우기"(미지정으로 되돌림).
    if let Some(slot) = recording_field.as_ref()
        && slot.field_id == RECORDING_FIELD
    {
        let idx = slot.idx;
        let value = match captured {
            KeyCapture::Combo(c) => Some(MigrationValue::Set(c.clone())),
            KeyCapture::Clear => Some(MigrationValue::Unset),
            KeyCapture::None => None,
        };
        if let Some(value) = value {
            if let Some(row) = state
                .preview
                .as_mut()
                .and_then(|p| p.migration.get_mut(idx))
            {
                row.value = value;
            }
            *recording_field = None;
        }
    }

    // 충돌 확인의 답.
    if let Some(accepted) = state.conflict_answer.take() {
        state.conflict_prompt = None;
        if accepted {
            state.apply(
                settings,
                plugin_draft,
                ConflictPolicy::UnbindOther,
                (snapshot, ctx),
            );
        }
    }

    let export_clicked = Cell::new(false);
    let import_clicked = Cell::new(false);
    let apply_clicked = Cell::new(false);
    let toggle_clicked = Cell::new(false);
    let choose_another = Cell::new(false);

    let current_overrides = merged_overrides(&ctx.overrides, plugin_draft);
    let back_clicked = {
        let labels = Labels {
            general: &settings.general,
            scripts: &settings.scripts,
            snapshot,
            names: &ctx.plugin_names,
        };
        let vm = state.preview.as_ref().map(|p| {
            build_view_model(p, state, &settings.keybindings, &current_overrides, &labels)
        });
        let failed = state.failure.is_some();
        let view = if vm.is_some() || failed {
            DrillDownView::Detail
        } else {
            DrillDownView::List
        };
        let (total, unresolved, picked) = vm
            .as_ref()
            .map_or((0, 0, 0), |vm| (vm.total, vm.unresolved, vm.picked));
        let changed_only = state.changed_only;

        let actions = |ui: &mut egui::Ui, th: &Theme| {
            if failed {
                return;
            }
            if Button::new(t("settings.keybindings.apply_button"))
                .variant(ButtonVariant::Primary)
                .size(ControlSize::Sm)
                .enabled(unresolved == 0 && picked > 0)
                .show(ui, th)
                .clicked()
            {
                apply_clicked.set(true);
            }
            if unresolved > 0 {
                ui.label(
                    egui::RichText::new(t_fmt(
                        "settings.keybindings.ie_unresolved_count",
                        &unresolved.to_string(),
                    ))
                    .monospace()
                    .size(th.font_size_caption.value())
                    .color(th.accent_warning()),
                );
            }
            let label = if changed_only {
                t_fmt("settings.keybindings.ie_show_all", &total.to_string())
            } else {
                t("settings.keybindings.ie_changed_only").to_string()
            };
            if Button::new(&label)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .show(ui, th)
                .clicked()
            {
                toggle_clicked.set(true);
            }
        };

        let failure = state.failure.as_ref();
        let out = DrillDown::new("settings_kb_import_export")
            .view(view)
            .title(t("settings.keybindings.ie_detail_title"))
            .back_label(t("settings.keybindings.preset_back"))
            .show(
                ui,
                &th,
                |ui, th| {
                    egui::Frame::new()
                        .inner_margin(tasty_ui_widgets::margin_sym(th.spacing_lg, th.spacing_md))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = th.spacing_md.value();
                            intro(
                                ui,
                                th,
                                th.measure_md,
                                t("settings.keybindings.ie_entry_intro"),
                            );
                            if action_row(
                                ui,
                                th,
                                icons::DOWNLOAD,
                                t("settings.keybindings.ie_export_title"),
                                t("settings.keybindings.ie_export_desc"),
                                t("settings.keybindings.ie_export_button"),
                                ButtonVariant::Secondary,
                            ) {
                                export_clicked.set(true);
                            }
                            if action_row(
                                ui,
                                th,
                                icons::FILE,
                                t("settings.keybindings.ie_import_title"),
                                t("settings.keybindings.ie_import_desc"),
                                t("settings.keybindings.ie_import_button"),
                                ButtonVariant::Primary,
                            ) {
                                import_clicked.set(true);
                            }
                        });
                },
                |ui, th| {
                    egui::Frame::new()
                        .inner_margin(tasty_ui_widgets::margin_all(th.spacing_lg))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = th.spacing_md.value();
                            if let Some(f) = failure {
                                if parse_failure(ui, th, f) {
                                    choose_another.set(true);
                                }
                                return;
                            }
                            let Some(vm) = vm.as_ref() else {
                                return;
                            };
                            intro(ui, th, th.measure_lg, &vm.intro);
                            if !vm.migration.is_empty() {
                                migrate_card(
                                    ui,
                                    th,
                                    vm,
                                    state_migration(&mut state.preview),
                                    recording_field,
                                    &labels,
                                );
                            }
                            if let Some(dropped) = &vm.dropped {
                                dropped_notice(ui, th, dropped);
                            }
                            diff_table(
                                ui,
                                th,
                                vm,
                                state.changed_only,
                                &mut state.collapsed,
                                &mut state.deselected,
                            );
                        });
                },
                Some(&actions),
            );
        out.back_clicked
    };

    if export_clicked.get() {
        state.request = Some(ImportExportRequest::Export);
    }
    if import_clicked.get() || choose_another.get() {
        state.failure = None;
        state.request = Some(ImportExportRequest::Import);
    }
    if toggle_clicked.get() {
        state.changed_only = !state.changed_only;
    }
    if back_clicked {
        state.preview = None;
        state.failure = None;
        if recording_field
            .as_ref()
            .is_some_and(|s| s.field_id == RECORDING_FIELD)
        {
            *recording_field = None;
        }
    }
    if apply_clicked.get() {
        state.apply(
            settings,
            plugin_draft,
            ConflictPolicy::Reject,
            (snapshot, ctx),
        );
    }
}

fn state_migration(preview: &mut Option<Preview>) -> &mut [MigrationRow] {
    match preview.as_mut() {
        Some(p) => &mut p.migration,
        None => &mut [],
    }
}

/// jsx `IeActionRow` — glyph · 제목(13 primary) + 설명(12 muted, measure-md) · trailing 버튼.
fn action_row(
    ui: &mut egui::Ui,
    th: &Theme,
    glyph: icons::Icon,
    title: &str,
    desc: &str,
    button: &str,
    variant: ButtonVariant,
) -> bool {
    let mut clicked = false;
    egui::Frame::new()
        .fill(th.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_default().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, th.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_md.value();
                glyph_at(ui, glyph, th.icon_glyph_size_md, th.text_muted().to_egui());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    clicked = Button::new(button)
                        .variant(variant)
                        .size(ControlSize::Sm)
                        .show(ui, th)
                        .clicked();
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y =
                            tasty_ui_widgets::tokens::STRUCT_GAP_2.value();
                        ui.label(
                            egui::RichText::new(title)
                                .size(th.font_size_body.value())
                                .color(th.text_primary()),
                        );
                        ui.scope(|ui| {
                            ui.set_max_width(th.measure_md.value());
                            ui.label(
                                egui::RichText::new(desc)
                                    .size(th.font_size_term_sm.value())
                                    .color(th.text_muted()),
                            );
                        });
                    });
                });
            });
        });
    clicked
}

/// jsx `IeDiffTable` — 4 열 grid(select · action · current · imported) + 그룹 헤더.
fn diff_table(
    ui: &mut egui::Ui,
    th: &Theme,
    vm: &ViewModel,
    changed_only: bool,
    collapsed: &mut BTreeSet<Group>,
    deselected: &mut BTreeSet<RowKey>,
) {
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
    let pad_x = th.spacing_md.value();
    let pad_y = th.spacing_sm.value();
    let hairline = egui::Stroke::new(th.border_width.value(), th.separator.to_egui());

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;

        // ── 열 헤더 (padding: 0 space-md space-sm) ──
        let head_font = egui::FontId::monospace(th.font_size_micro.value());
        let head_h = ui.fonts(|f| f.row_height(&head_font)) + pad_y;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, head_h), egui::Sense::hover());
        let headers = [
            String::new(),
            t("settings.keybindings.preset_col_action").to_uppercase(),
            t("settings.keybindings.preset_col_before").to_uppercase(),
            t("settings.keybindings.ie_col_imported").to_uppercase(),
        ];
        for (i, text) in headers.iter().enumerate() {
            let g = truncated(
                ui,
                text,
                head_font.clone(),
                th.text_muted().to_egui(),
                (col_w[i] - pad_x * 2.0).max(0.0),
            );
            ui.painter().galley(
                egui::pos2(rect.left() + x_off[i] + pad_x, rect.top()),
                g,
                egui::Color32::PLACEHOLDER,
            );
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - th.border_width.value() * 0.5,
            hairline,
        );

        for g in &vm.groups {
            let shown: Vec<&RowView> = g
                .rows
                .iter()
                .filter(|r| !changed_only || r.changed)
                .collect();
            let open = !collapsed.contains(&g.group);
            group_header(ui, th, w, g, &shown, open, collapsed, deselected);
            if !open {
                continue;
            }
            for r in shown {
                let row_h = diff_row_height(ui, th, r);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::hover());
                // 선택 열 — `checkbox` 는 빈 라벨에도 박스 뒤 gap 을 차지하므로 박스 중심이
                // 열 중심에 오도록 시작점을 잡는다.
                let box_sz = th.checkbox_size().value();
                let sel_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + (col_w[0] - box_sz) * 0.5, rect.top()),
                    egui::vec2(col_w[0], row_h),
                );
                let mut on = !deselected.contains(&r.key);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(sel_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        if checkbox(ui, th, &mut on, "", true).changed() {
                            if on {
                                deselected.remove(&r.key);
                            } else {
                                deselected.insert(r.key.clone());
                            }
                        }
                    },
                );
                action_cell(ui, th, rect, x_off[1] + pad_x, col_w[1] - pad_x * 2.0, r);
                let mono = egui::FontId::monospace(th.font_size_term_sm.value());
                value_cell(
                    ui,
                    rect,
                    x_off[2] + pad_x,
                    col_w[2] - pad_x * 2.0,
                    &r.cur,
                    mono.clone(),
                    th.text_muted().to_egui(),
                );
                let fg = if r.blocked {
                    th.accent_warning()
                } else if r.changed {
                    th.accent_primary()
                } else {
                    th.text_muted()
                };
                value_cell(
                    ui,
                    rect,
                    x_off[3] + pad_x,
                    col_w[3] - pad_x * 2.0,
                    &r.next,
                    mono,
                    fg.to_egui(),
                );
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - th.border_width.value() * 0.5,
                    hairline,
                );
            }
        }
    });
}

/// 그룹 헤더 — 네 열을 가로지르는 한 행(surface-raised). select-all · chevron · 그룹명 ·
/// `N changed · M total`. 열 헤더를 반복하지 않는다.
#[allow(clippy::too_many_arguments)]
fn group_header(
    ui: &mut egui::Ui,
    th: &Theme,
    w: f32,
    g: &GroupView,
    shown: &[&RowView],
    open: bool,
    collapsed: &mut BTreeSet<Group>,
    deselected: &mut BTreeSet<RowKey>,
) {
    let h =
        th.checkbox_size().value().max(th.font_size_micro.value()) + th.spacing_sm.value() * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, th.surface_raised().to_egui());
    let inner = rect.shrink2(egui::vec2(th.spacing_md.value(), 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            let mut all = !shown.is_empty() && shown.iter().all(|r| !deselected.contains(&r.key));
            if checkbox(ui, th, &mut all, "", true).changed() {
                for r in &g.rows {
                    if all {
                        deselected.remove(&r.key);
                    } else {
                        deselected.insert(r.key.clone());
                    }
                }
            }
            // chevron + 그룹명 — 한 버튼(접힘 토글).
            let glyph = th.icon_glyph_size_sm.value();
            let galley = ui.painter().layout_no_wrap(
                t(g.group.label_key()).to_uppercase(),
                egui::FontId::monospace(th.font_size_micro.value()),
                th.text_secondary().to_egui(),
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
                .image(glyph, th.text_muted().to_egui())
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
                    collapsed.insert(g.group);
                } else {
                    collapsed.remove(&g.group);
                }
            }
            let changed = g.rows.iter().filter(|r| r.changed).count();
            ui.label(
                egui::RichText::new(t_fmt2(
                    "settings.keybindings.ie_group_counts",
                    &changed.to_string(),
                    &g.rows.len().to_string(),
                ))
                .monospace()
                .size(th.font_size_caption.value())
                .color(th.text_muted()),
            );
        },
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - th.border_width.value() * 0.5,
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
}

fn diff_row_height(ui: &egui::Ui, th: &Theme, r: &RowView) -> f32 {
    let body = ui.fonts(|f| f.row_height(&egui::FontId::proportional(th.font_size_body.value())));
    let sub = if r.sub.is_some() {
        ui.fonts(|f| f.row_height(&egui::FontId::proportional(th.font_size_micro.value())))
            + tasty_ui_widgets::tokens::STRUCT_GAP_2.value()
    } else {
        0.0
    };
    body + sub + th.spacing_sm.value() * 2.0
}

fn action_cell(ui: &egui::Ui, th: &Theme, rect: egui::Rect, x: f32, max_w: f32, r: &RowView) {
    let body_font = egui::FontId::proportional(th.font_size_body.value());
    let title = truncated(
        ui,
        &r.action,
        body_font,
        th.text_secondary().to_egui(),
        max_w,
    );
    let gap = tasty_ui_widgets::tokens::STRUCT_GAP_2.value();
    let sub_h = if r.sub.is_some() {
        ui.fonts(|f| f.row_height(&egui::FontId::proportional(th.font_size_micro.value()))) + gap
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
    let sub_y = top + title_h + gap;
    match &r.sub {
        Some(Sub::Plugin(name)) => {
            let d = th.status_dot_size.value();
            let micro = egui::FontId::monospace(th.font_size_micro.value());
            let g = truncated(ui, name, micro, th.text_muted().to_egui(), max_w - d);
            let cy = sub_y + g.rect.height() * 0.5;
            ui.painter().circle_filled(
                egui::pos2(rect.left() + x + d * 0.5, cy),
                d * 0.5,
                th.accent_agent().to_egui(),
            );
            ui.painter().galley(
                egui::pos2(rect.left() + x + d + PLUGIN_DOT_GAP.value(), sub_y),
                g,
                egui::Color32::PLACEHOLDER,
            );
        }
        Some(Sub::Note(note)) => {
            let g = truncated(
                ui,
                note,
                egui::FontId::proportional(th.font_size_micro.value()),
                th.text_muted().to_egui(),
                max_w,
            );
            ui.painter().galley(
                egui::pos2(rect.left() + x, sub_y),
                g,
                egui::Color32::PLACEHOLDER,
            );
        }
        None => {}
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

/// jsx `IeMigrateCard` — 톤 틴트 카드(헤더 · 설명 · 행들).
fn migrate_card(
    ui: &mut egui::Ui,
    th: &Theme,
    vm: &ViewModel,
    rows: &mut [MigrationRow],
    recording_field: &mut Option<RecordingSlot>,
    labels: &Labels<'_>,
) {
    let total = rows.len();
    let left = vm.unresolved;
    let done = left == 0;
    let tone = if done {
        th.accent_success().to_egui()
    } else {
        th.accent_warning().to_egui()
    };
    egui::Frame::new()
        .fill(tone.gamma_multiply(MIGRATE_CARD_FILL))
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            tone.gamma_multiply(MIGRATE_CARD_BORDER),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, th.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                let glyph = if done {
                    icons::CHECK
                } else {
                    icons::ALERT_TRIANGLE
                };
                glyph_at(ui, glyph, th.icon_glyph_size_md, tone);
                ui.label(
                    egui::RichText::new(if done {
                        t("settings.keybindings.ie_migrate_title_done")
                    } else {
                        t("settings.keybindings.ie_migrate_title_pending")
                    })
                    .size(th.font_size_body.value())
                    .color(tone),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let counter = if done {
                        t_fmt2(
                            "settings.keybindings.ie_migrate_count_done",
                            &total.to_string(),
                            &total.to_string(),
                        )
                    } else {
                        t_fmt2(
                            "settings.keybindings.ie_migrate_count_pending",
                            &left.to_string(),
                            &total.to_string(),
                        )
                    };
                    ui.label(
                        egui::RichText::new(counter)
                            .monospace()
                            .size(th.font_size_caption.value())
                            .color(tone),
                    );
                });
            });
            ui.scope(|ui| {
                ui.set_max_width(th.measure_lg.value());
                ui.label(
                    egui::RichText::new(if done {
                        t("settings.keybindings.ie_migrate_desc_done")
                    } else {
                        t("settings.keybindings.ie_migrate_desc_pending")
                    })
                    .size(th.font_size_term_sm.value())
                    .color(th.text_secondary()),
                );
            });
            vspace(ui, th.spacing_xs);
            for (i, (row, view)) in rows.iter_mut().zip(&vm.migration).enumerate() {
                migrate_row(ui, th, i, row, view, recording_field, labels);
            }
        });
}

/// jsx `IeMigrateRow` — 라벨 288 · 원래 조합 120 · → · 위젯 · trailing + 부제(충돌 · fan-out).
fn migrate_row(
    ui: &mut egui::Ui,
    th: &Theme,
    index: usize,
    row: &mut MigrationRow,
    view: &MigrationView,
    recording_field: &mut Option<RecordingSlot>,
    labels: &Labels<'_>,
) {
    let recording = recording_field
        .as_ref()
        .is_some_and(|s| s.field_id == RECORDING_FIELD && s.idx == index);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let w = ui.available_width();
        let (sep, _) =
            ui.allocate_exact_size(egui::vec2(w, th.border_width.value()), egui::Sense::hover());
        ui.painter().hline(
            sep.x_range(),
            sep.center().y,
            egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
        );
        vspace(ui, th.spacing_sm);
        ui.horizontal(|ui| {
            ui.set_min_height(th.item_height_interactive.value());
            ui.spacing_mut().item_spacing.x = th.spacing_md.value();
            fixed_label(
                ui,
                LABEL_COL_WIDTH,
                &view.action,
                egui::FontId::proportional(th.font_size_body.value()),
                th.text_secondary().to_egui(),
            );
            fixed_label(
                ui,
                MIGRATE_FROM_W,
                &view.from,
                egui::FontId::monospace(th.font_size_term_sm.value()),
                th.text_muted().to_egui(),
            );
            glyph_at(
                ui,
                icons::CHEVRON_RIGHT,
                th.icon_glyph_size_sm,
                th.text_muted().to_egui(),
            );
            match view.kind {
                ReplacementKind::ModifierCombo => {
                    let combos: Vec<String> = all_modifier_combos()
                        .into_iter()
                        .map(|c| c.name())
                        .filter(|n| !n.contains("option"))
                        .collect();
                    let unset = row.value == MigrationValue::Unset;
                    let mut options: Vec<String> = Vec::new();
                    if unset {
                        options.push(t("settings.keybindings.ie_pick_modifier").to_string());
                    }
                    options.extend(combos.iter().map(|n| labels.combo(n)));
                    let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
                    let base = usize::from(unset);
                    let mut idx = match &row.value {
                        MigrationValue::Set(v) => {
                            combos.iter().position(|n| n == v).map_or(0, |p| p + base)
                        }
                        _ => 0,
                    };
                    if select(
                        ui,
                        th,
                        &format!("kb_ie_modifier_{index}"),
                        &mut idx,
                        &option_refs,
                        th.field_width_md.value(),
                        true,
                    ) && idx >= base
                        && let Some(name) = combos.get(idx - base)
                    {
                        row.value = MigrationValue::Set(name.clone());
                    }
                }
                ReplacementKind::Combo => {
                    if record_slot(ui, th, row, view, recording, labels) {
                        *recording_field = Some(RecordingSlot {
                            field_id: RECORDING_FIELD.to_string(),
                            idx: index,
                            field_kind: FieldKind::Combo,
                        });
                    }
                }
            }
            match (&row.value, &view.conflict) {
                (_, Some(_)) => {}
                (MigrationValue::Set(_), None) => glyph_at(
                    ui,
                    icons::CHECK,
                    th.icon_glyph_size_sm,
                    th.accent_success().to_egui(),
                ),
                (MigrationValue::Unbound, None) => {
                    tag(
                        ui,
                        th,
                        t("settings.keybindings.ie_unbound_tag"),
                        TagVariant::Default,
                        false,
                    );
                }
                (MigrationValue::Unset, None) => {
                    ui.label(
                        egui::RichText::new(t("settings.keybindings.ie_not_set"))
                            .size(th.font_size_caption.value())
                            .color(th.accent_warning()),
                    );
                    // 축 modifier 는 비울 수 없다 — "비워 두기" 는 콤보 자리에만.
                    if view.kind == ReplacementKind::Combo
                        && Button::new(t("settings.keybindings.ie_leave_unbound"))
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, th)
                            .clicked()
                    {
                        row.value = MigrationValue::Unbound;
                        if recording {
                            *recording_field = None;
                        }
                    }
                }
            }
        });
        if let Some(conflict) = &view.conflict {
            vspace(ui, th.spacing_xs);
            ui.horizontal(|ui| {
                ui.add_space(LABEL_COL_WIDTH.value());
                ui.spacing_mut().item_spacing.x = GROUP_CHEVRON_GAP.value();
                glyph_at(
                    ui,
                    icons::ALERT_TRIANGLE,
                    th.icon_glyph_size_sm,
                    th.accent_danger().to_egui(),
                );
                ui.label(
                    egui::RichText::new(conflict)
                        .size(th.font_size_caption.value())
                        .color(th.accent_danger()),
                );
            });
        } else if let Some(fanout) = &view.fanout {
            vspace(ui, th.spacing_xs);
            ui.horizontal(|ui| {
                ui.add_space(LABEL_COL_WIDTH.value());
                ui.label(
                    egui::RichText::new(fanout)
                        .size(th.font_size_caption.value())
                        .color(th.text_muted()),
                );
            });
        }
        vspace(ui, th.spacing_sm);
    });
}

/// 녹화 슬롯 — min 140 × 24 · mono · surface-raised · 충돌이면 danger 테두리 · 빈 값은
/// text-disabled. 클릭하면 녹화를 시작한다.
fn record_slot(
    ui: &mut egui::Ui,
    th: &Theme,
    row: &MigrationRow,
    view: &MigrationView,
    recording: bool,
    labels: &Labels<'_>,
) -> bool {
    let font = egui::FontId::monospace(th.font_size_term_sm.value());
    let (text, empty) = if recording {
        (t("settings.keybindings.hint_press_key").to_string(), false)
    } else {
        match &row.value {
            MigrationValue::Set(c) => (labels.combo(c), false),
            MigrationValue::Unbound => (t("settings.keybindings.ie_unbound").to_string(), false),
            MigrationValue::Unset => (t("settings.keybindings.ie_not_set").to_string(), true),
        }
    };
    let fg = if empty {
        th.text_disabled()
    } else {
        th.text_primary()
    };
    let galley = ui.painter().layout_no_wrap(text, font, fg.to_egui());
    let pad = th.spacing_sm.value();
    let w = (galley.rect.width() + pad * 2.0).max(RECORD_SLOT_MIN_W.value());
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(w, RECORD_SLOT_H.value()), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let border = if view.conflict.is_some() {
        th.accent_danger()
    } else if recording {
        th.accent_primary()
    } else {
        th.border_default()
    };
    ui.painter().rect(
        rect,
        th.corner_radius.value(),
        th.surface_raised().to_egui(),
        egui::Stroke::new(th.border_width.value(), border.to_egui()),
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
    resp.clicked()
}

/// 버린 plugin override 안내 — 정보(경고 아님): 잘못된 것도 할 일도 없다.
fn dropped_notice(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        glyph_at(
            ui,
            icons::HELP_CIRCLE,
            th.icon_glyph_size_sm,
            th.text_muted().to_egui(),
        );
        ui.label(
            egui::RichText::new(text)
                .size(th.font_size_term_sm.value())
                .color(th.text_muted()),
        );
    });
}

/// 파싱 실패 — 보던 대상에 대한 사실이라 toast/popup 이 아니라 detail 영역 안에 인라인.
/// "다른 파일 고르기" 가 눌리면 true.
fn parse_failure(ui: &mut egui::Ui, th: &Theme, failure: &Failure) -> bool {
    let danger = th.accent_danger().to_egui();
    let mut clicked = false;
    egui::Frame::new()
        .fill(danger.gamma_multiply(FAILURE_FILL))
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            danger.gamma_multiply(FAILURE_BORDER),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_sym(CARD_PAD_X, th.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                glyph_at(ui, icons::ALERT_CIRCLE, th.icon_glyph_size_md, danger);
                ui.label(
                    egui::RichText::new(t("settings.keybindings.ie_failure_title"))
                        .size(th.font_size_body.value())
                        .color(danger),
                );
            });
            let body = match failure.line {
                Some(line) => t_fmt2(
                    "settings.keybindings.ie_failure_body_line",
                    &failure.path,
                    &line.to_string(),
                ),
                None => t_fmt("settings.keybindings.ie_failure_body", &failure.path),
            };
            ui.scope(|ui| {
                ui.set_max_width(th.measure_md.value());
                ui.label(
                    egui::RichText::new(body)
                        .size(th.font_size_term_sm.value())
                        .color(th.text_secondary()),
                );
            });
            vspace(ui, th.spacing_xs);
            clicked = Button::new(t("settings.keybindings.ie_choose_another"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .show(ui, th)
                .clicked();
        });
    clicked
}

/// 안내문 — 12 muted, max-width `measure`.
fn intro(ui: &mut egui::Ui, th: &Theme, measure: LogicalPx, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(measure.value());
        ui.label(
            egui::RichText::new(text)
                .size(th.font_size_term_sm.value())
                .color(th.text_muted()),
        );
    });
}

fn glyph_at(ui: &mut egui::Ui, glyph: icons::Icon, size: LogicalPx, tint: egui::Color32) {
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
