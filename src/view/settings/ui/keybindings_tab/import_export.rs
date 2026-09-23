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
//! 근거는 `docs/adr/0619-keybinding-settings-and-hints.md`.
//!
//! 모듈 경계는 **상태·계산 대 화면 단위 그리기**다. 이 파일은 서브탭 상태와 그 전이(export ·
//! import · apply)와 진입 함수를 들고, 계산은 `model`(행 모델) · `labels`(표시 문자열) ·
//! `view_model`(한 프레임의 표시값) · `bundle_notices`(경고 블록에 오를 줄)가, 그리기는 jsx 컴포넌트 단위로 `entry`(`IeActionRow`) ·
//! `diff_table`(`IeDiffTable`) · `migrate`(`IeMigrateCard`/`IeMigrateRow`) · `notices` 가,
//! 그 넷이 함께 쓰는 칠하기 헬퍼는 `paint` 가 든다. 치수 상수는 여기 남는다 — 갤러리 짝과의
//! 값 일치를 보는 가드가 이 경로에서 읽는다.

mod bundle_notices;
mod diff_table;
mod entry;
mod labels;
mod migrate;
mod model;
mod notices;
mod paint;
mod view_model;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tasty_host_plugin::keybinding_bundle::option_migration::{
    ConflictPolicy, MigrationError, TargetOs, resolve_migration, scan_option_bindings,
};
use tasty_host_plugin::keybinding_bundle::{
    BundleError, BundleWarning, DecodeEnv, DecodedBundle, PluginShortcutOverrides, decode, encode,
};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, DrillDown, DrillDownView};

use crate::adapters::ui::icons;
use crate::i18n::{t, t_fmt, t_fmt2};
use crate::settings::{KeybindingSettings, Settings};
use crate::settings_ui::PluginShortcutSnapshot;

pub(crate) use model::PluginShortcutDraft;
use model::{
    Group, MigrationRow, MigrationValue, RowKey, apply_rows, merged_overrides, plan_of, row_keys,
};

use super::{KeyCapture, RecordingSlot};
use diff_table::diff_table;
use entry::action_row;
use labels::Labels;
use migrate::migrate_card;
use notices::{ExportFailureAction, bundle_notices, dropped_notice, export_failure, parse_failure};
use paint::intro;
use view_model::build_view_model;

// 아래 치수 중 32 · 120 은 `size-*` 스케일 위의 값이다(140 은 스케일에 아직 없다). 디자인은
// 이 셋에 컴포넌트 토큰을 열었지만(`kb-ie-select-column-width` · `kb-ie-from-column-width` ·
// `kb-ie-slot-min-width`) vendor 한 DTCG export 에는 아직 그 이름이 없어 읽을 수 없다 — 그래서
// 지금은 명명 상수다(갤러리 `kb_import_export.rs` 와 짝 — 두 값의 일치는
// `gallery_copied_dimensions` 가 본다). 같은 회신이 연 토큰 중 semantic 별칭인 둘은 이미 읽는다:
// 카드 inset 은 `space-md`(14 에서 스냅, 양축), 슬롯 높이는 `control-height-tab`
// (`item_height_tab`).
/// 표 선택 열 — 디자인 `--tasty-kb-ie-select-column-width`(→ `size-32`).
const SELECT_COL_W: LogicalPx = LogicalPx(32.0);
/// 마이그레이션 행 원래 조합 열 — 디자인 `--tasty-kb-ie-from-column-width`(→ `size-120`).
const MIGRATE_FROM_W: LogicalPx = LogicalPx(120.0);
/// 녹화 슬롯 최소 폭 — 디자인 `--tasty-kb-ie-slot-min-width`(→ `size-140`).
const RECORD_SLOT_MIN_W: LogicalPx = LogicalPx(140.0);
/// 그룹 헤더 chevron ↔ 그룹명, 충돌 부제 아이콘 ↔ 문구, 경고 줄 글머리 ↔ 문구 간격 — jsx `gap: 6`.
const GROUP_CHEVRON_GAP: LogicalPx = LogicalPx(6.0);
/// plugin 행 부제의 점 ↔ plugin 이름 간격 — jsx `gap: 5`.
const PLUGIN_DOT_GAP: LogicalPx = LogicalPx(5.0);

/// 충돌 개수 줄이 서는 최소 충돌 수 — 하나일 때는 행의 인라인 이유가 혼자 싣는다.
const CONFLICT_SUMMARY_FROM: usize = 2;

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
    /// 경고 블록에 오를 줄(고정 순서).
    notices: Vec<bundle_notices::BundleNotice>,
    migration: Vec<MigrationRow>,
}

/// 읽지 못한 파일.
struct Failure {
    path: String,
    line: Option<usize>,
}

/// 쓰지 못한 내보내기 — 재시도가 같은 경로를 다시 쓴다.
struct ExportFailure {
    path: PathBuf,
    reason: ExportFailReason,
}

/// 내보내기 실패의 이유 — 블록 본문 **가운데 구절**을 고른다. 앞뒤 문장(경로 · "Nothing was
/// written.")은 갈래와 무관하게 같다.
///
/// 갈래가 고정 집합인 이유: 가운데 구절은 세 언어에서 같은 문장으로 읽혀야 하므로 OS 가 낸
/// 문장이 그 자리에 올 수 없다. 알아볼 수 있는 셋만 이름을 갖고 나머지는 catch-all 하나로
/// 접힌다 — 그 갈래에서만 OS 문장이 **본문 아래 제 줄**로 따라간다.
enum ExportFailReason {
    /// 파일시스템이 읽기 전용이다.
    ReadOnly,
    /// 쓸 권한이 없다.
    PermissionDenied,
    /// 볼륨이 찼다.
    DiskFull,
    /// 알아볼 수 없는 실패 — 구절은 고정이고, 담은 문장은 아래 줄로 나간다.
    Unknown(String),
}

impl ExportFailReason {
    fn of_io(e: &std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::ReadOnlyFilesystem => ExportFailReason::ReadOnly,
            std::io::ErrorKind::PermissionDenied => ExportFailReason::PermissionDenied,
            // `ErrorKind::StorageFull` 은 아직 nightly 다. `crate::db::classify_io` 와 같은
            // 우회로 raw OS 코드를 본다 — 상수는 그쪽 하나만 둔다.
            _ if e.raw_os_error() == Some(crate::db::disk_full_os_error()) => {
                ExportFailReason::DiskFull
            }
            _ => ExportFailReason::Unknown(e.to_string()),
        }
    }

    /// 본문 아래 제 줄에 실을 OS 문장 — catch-all 갈래에만 있다.
    fn os_message(&self) -> Option<&str> {
        match self {
            ExportFailReason::Unknown(message) => Some(message.as_str()),
            _ => None,
        }
    }
}

/// 서브탭 상태 — `SettingsUiState` 가 소유한다.
pub struct ImportExportState {
    preview: Option<Preview>,
    failure: Option<Failure>,
    /// 내보내기 실패 — 떠 있는 동안 Export 버튼이 꺼진다.
    export_failure: Option<ExportFailure>,
    /// 경고 블록의 접힌 줄을 펼쳤는가.
    notices_expanded: bool,
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
            export_failure: None,
            notices_expanded: false,
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
            .map_err(|e| {
                tracing::error!("keybinding export: encode failed: {e}");
                ExportFailReason::Unknown(e.to_string())
            })
            .and_then(|text| {
                std::fs::write(path, text).map_err(|e| {
                    tracing::error!("keybinding export to {} failed: {e}", path.display());
                    ExportFailReason::of_io(&e)
                })
            });
        match written {
            Ok(()) => {
                self.export_failure = None;
                self.toast = Some(t_fmt(
                    "settings.keybindings.ie_exported_toast",
                    &path.display().to_string(),
                ));
            }
            Err(reason) => {
                self.export_failure = Some(ExportFailure {
                    path: path.to_path_buf(),
                    reason,
                });
            }
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
        let notices = bundle_notices::bundle_notices(&decoded.warnings);
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
            notices,
            migration,
        });
        self.notices_expanded = false;
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

/// 버린 plugin 을 정보 줄 몫으로 모은다. 화면 어디에도 안 오르는 경고는 로그로만 남는다.
fn dropped_plugins(path: &Path, warnings: &[BundleWarning]) -> Vec<(String, usize)> {
    let mut dropped = Vec::new();
    for w in warnings {
        if let BundleWarning::DroppedUninstalledPlugin {
            plugin_id,
            commands,
        } = w
        {
            dropped.push((plugin_id.clone(), *commands));
        }
        if !bundle_notices::is_shown(w) {
            // 이 경고들의 표시 문구는 디자인이 정하지 않았다 — 자리를 비워 두고 로그만 남긴다.
            tracing::warn!("keybinding import {}: {w}", path.display());
        }
    }
    dropped
}
// reason: 설정 draft · plugin draft · 녹화 슬롯 · 이 화면 상태는 호출부가 서로 다른 필드에서 따로
// 빌리는 가변 참조다 — 한 구조체로 묶으면 `draw_keybindings_tab` 의 다른 서브탭 갈래와 빌림이 겹친다.
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
    let show_more_notices = Cell::new(false);
    let export_action = Cell::new(None::<ExportFailureAction>);

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
        let export_fail = state.export_failure.as_ref();
        let notices_expanded = state.notices_expanded;
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
                            let mut export_notice = |ui: &mut egui::Ui| {
                                if let Some(f) = export_fail
                                    && let Some(action) = export_failure(ui, th, f)
                                {
                                    export_action.set(Some(action));
                                }
                            };
                            if action_row(
                                ui,
                                th,
                                icons::DOWNLOAD,
                                t("settings.keybindings.ie_export_title"),
                                t("settings.keybindings.ie_export_desc"),
                                t("settings.keybindings.ie_export_button"),
                                ButtonVariant::Secondary,
                                export_fail.is_none(),
                                export_fail
                                    .map(|_| &mut export_notice as &mut dyn FnMut(&mut egui::Ui)),
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
                                true,
                                None,
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
                            if !vm.notices.is_empty()
                                && bundle_notices(ui, th, &vm.notices, notices_expanded)
                            {
                                show_more_notices.set(true);
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
    match export_action.get() {
        Some(ExportFailureAction::Retry) => {
            if let Some(path) = state.export_failure.as_ref().map(|f| f.path.clone()) {
                state.export_to(&path, &settings.keybindings, ctx, plugin_draft);
            }
        }
        Some(ExportFailureAction::ChooseAnother) => {
            state.export_failure = None;
            state.request = Some(ImportExportRequest::Export);
        }
        None => {}
    }
    if show_more_notices.get() {
        state.notices_expanded = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    // 부르는 테스트가 권한 비트를 쓰는 unix 전용 하나뿐이라 같은 cfg 로 가른다 —
    // 안 가르면 Windows 의 lib test 가 dead_code 로 컴파일되지 않는다.
    #[cfg(unix)]
    fn export(state: &mut ImportExportState, path: &Path) {
        state.export_to(
            path,
            &KeybindingSettings::default(),
            &PluginBundleContext::default(),
            &PluginShortcutDraft::new(),
        );
    }

    /// 쓰지 못하면 toast 가 아니라 실패 블록이 선다. 디렉토리에서 쓰기 비트를 떼면 OS 는
    /// `EACCES` 를 낸다 — 파일시스템이 읽기 전용인 것과 **다른 갈래**이고, 디자인이 그 둘에
    /// 서로 다른 구절을 준다. 같은 경로가 다시 쓰일 수 있게 되면 성공이 블록을 걷는다
    /// (재시도가 닫는 경로).
    #[cfg(unix)]
    #[test]
    fn a_failed_export_raises_the_inline_block_and_a_later_success_clears_it() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("임시 디렉토리");
        let path = dir.path().join("kb.toml");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
            .expect("읽기 전용으로");
        let mut state = ImportExportState::default();
        export(&mut state, &path);
        // root 로 돌면 권한이 안 막는다 — 그때는 이 단정이 무엇도 재지 않으므로 멈춘다.
        if std::fs::metadata(&path).is_ok() {
            return;
        }
        assert!(state.toast.is_none(), "실패인데 toast 가 섰다");
        let failure = state.export_failure.as_ref().expect("실패 블록이 없다");
        assert_eq!(failure.path, path);
        assert!(matches!(failure.reason, ExportFailReason::PermissionDenied));
        assert!(
            failure.reason.os_message().is_none(),
            "알아본 갈래인데 OS 문장 줄이 딸려 나온다"
        );

        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("쓰기 가능으로");
        export(&mut state, &path);
        assert!(
            state.export_failure.is_none(),
            "성공했는데 실패 블록이 남았다"
        );
        assert!(state.take_toast().is_some(), "성공 toast 가 없다");
    }

    /// 가운데 구절은 **고정 집합**이다 — 알아본 셋은 저마다 갈래를 갖고 OS 문장을 달지
    /// 않으며, 나머지 하나만 catch-all 로 접히면서 그 문장을 아래 줄로 들고 간다.
    #[test]
    fn each_recognised_cause_takes_its_own_clause_and_only_the_catch_all_carries_the_os_text() {
        use std::io::{Error, ErrorKind};

        let read_only = ExportFailReason::of_io(&Error::from(ErrorKind::ReadOnlyFilesystem));
        assert!(matches!(read_only, ExportFailReason::ReadOnly));
        assert!(read_only.os_message().is_none());

        let denied = ExportFailReason::of_io(&Error::from(ErrorKind::PermissionDenied));
        assert!(matches!(denied, ExportFailReason::PermissionDenied));
        assert!(denied.os_message().is_none());

        let full =
            ExportFailReason::of_io(&Error::from_raw_os_error(crate::db::disk_full_os_error()));
        assert!(
            matches!(full, ExportFailReason::DiskFull),
            "볼륨이 찬 것을 알아보지 못했다 — `ErrorKind::StorageFull` 이 nightly 라 raw 코드로 본다"
        );
        assert!(full.os_message().is_none());

        let unknown = ExportFailReason::of_io(&Error::from(ErrorKind::InvalidData));
        assert!(matches!(unknown, ExportFailReason::Unknown(_)));
        assert!(
            unknown.os_message().is_some(),
            "알아보지 못한 갈래인데 OS 문장이 사라졌다 — 그러면 사용자가 볼 단서가 없다"
        );
    }
}
