//! Settings UI 의 `FileHandler` 탭 (Phase D MD3 + Phase E ME4).
//!
//! 4 개의 sub-tab:
//! - **Detectors** — 등록된 detector 의 목록. Enabled 토글, user-origin 항목 삭제,
//!   user 추가 (id + 확장자 list 기반 간단 form). 다른 rule kind 는 TOML 손편집으로.
//! - **Handlers** — 등록된 handler 의 목록. Enabled 토글, user-origin 항목 삭제,
//!   user 추가 (id + detector dropdown + priority + action kind/params).
//! - **Extension Mapping** — 같은 확장자를 광고하는 여러 detector 의 우선순위 표 편집.
//! - **Recent picks** — picker 가 기록한 LRU 목록 + Forget.
//!
//! 편집 사항은 `FileHandlerEditDraft` 에 쌓이고 Settings 의 Save 버튼이 registry 에
//! commit + `~/.tasty/file-handlers.toml` 에 atomic write 한다.

use std::collections::{BTreeMap, BTreeSet};

use crate::file_format::{
    DetectorDecl, DetectorId, DetectorInfo, DetectorRuleDecl, DetectorRuleKind, FileFormatRegistry,
    RuleOrigin,
};
use crate::file_handler::{
    FileHandlerRegistry, HandlerAction, HandlerId, HandlerOwner, UserHandlerActionDecl,
    UserHandlerUpsertDecl,
};
use crate::file_handler_recent::RecentPicks;
use crate::i18n::t;

/// FileHandler 탭의 sub-tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileHandlerSubTab {
    Detectors,
    Handlers,
    ExtensionMapping,
    RecentPicks,
}

/// Detectors / Handlers sub-tab 의 편집 draft. Save 시 `apply` 가 호출되고 Cancel 시 폐기.
///
/// 모델: "사용자의 의도" — disabled 의 want_enabled, 삭제 want_remove, 추가 add_*.
/// 현재 registry 상태와 비교하지 않고 항상 명시적 user-origin override 로 commit.
#[derive(Debug, Clone, Default)]
pub(crate) struct FileHandlerEditDraft {
    /// detector id → 사용자가 원하는 enabled 상태. 없으면 변경 없음.
    detector_enabled: BTreeMap<DetectorId, bool>,
    handler_enabled: BTreeMap<HandlerId, bool>,
    /// user-origin entry 삭제 의도.
    remove_detector: BTreeSet<DetectorId>,
    remove_handler: BTreeSet<HandlerId>,
    /// 새로 추가될 user detector / handler.
    add_detector: Vec<DetectorDecl>,
    add_handler: Vec<UserHandlerUpsertDecl>,
    /// "+ Add user detector" / "+ Add user handler" inline form 상태.
    add_detector_form: AddDetectorForm,
    add_handler_form: AddHandlerForm,
}

impl FileHandlerEditDraft {
    pub fn has_changes(&self) -> bool {
        !self.detector_enabled.is_empty()
            || !self.handler_enabled.is_empty()
            || !self.remove_detector.is_empty()
            || !self.remove_handler.is_empty()
            || !self.add_detector.is_empty()
            || !self.add_handler.is_empty()
    }

    pub fn apply(
        self,
        file_format: &FileFormatRegistry,
        file_handler: &FileHandlerRegistry,
    ) {
        for (id, enabled) in &self.detector_enabled {
            // enabled = true 인데 detector 가 host/plugin default 로 이미 enabled 면 user
            // override 를 굳이 추가하지 않는다 (불필요한 user contribution 회피). 그러나
            // 현재 상태를 정확히 모르므로 단순화: 항상 명시적으로 set, 동일 값이면 patch
            // semantics 상 no-op.
            file_format.set_user_detector_disabled(id, !enabled);
        }
        for (id, enabled) in &self.handler_enabled {
            file_handler.set_user_handler_disabled(id, !enabled);
        }
        for id in &self.remove_detector {
            file_format.remove_user_detector(id);
        }
        for id in &self.remove_handler {
            file_handler.remove_user_handler(id);
        }
        for decl in self.add_detector {
            if let Err(e) = file_format.upsert_user_detector(decl) {
                tracing::warn!("file_handler tab: upsert_user_detector failed: {e}");
            }
        }
        for decl in self.add_handler {
            if let Err(e) = file_handler.upsert_user_handler(decl) {
                tracing::warn!("file_handler tab: upsert_user_handler failed: {e}");
            }
        }
    }
}

/// "+ Add user detector" inline form 의 입력 상태.
#[derive(Debug, Clone, Default)]
struct AddDetectorForm {
    open: bool,
    id_input: String,
    /// 콤마 또는 공백 구분의 확장자 목록.
    extensions_input: String,
    /// 단일 path-glob 패턴 (옵션).
    path_glob_input: String,
    error: Option<String>,
}

/// "+ Add user handler" inline form 의 입력 상태.
#[derive(Debug, Clone, Default)]
struct AddHandlerForm {
    open: bool,
    short_name_input: String,
    detector_id_input: String,
    priority_input: String, // 비우면 default 100
    action_kind: AddHandlerActionKind,
    action_surface_kind: String,
    action_param_key: String,
    action_ipc_method: String,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddHandlerActionKind {
    OpenSurface,
    Ipc,
    System,
}

impl Default for AddHandlerActionKind {
    fn default() -> Self {
        Self::OpenSurface
    }
}

pub(crate) fn draw_file_handler_tab(
    ui: &mut egui::Ui,
    sub_tab: &mut FileHandlerSubTab,
    draft: &mut Option<BTreeMap<String, Vec<DetectorId>>>,
    new_ext_input: &mut String,
    recent_picks: &mut Option<RecentPicks>,
    fh_draft: &mut FileHandlerEditDraft,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
    recent_picks_path: Option<&std::path::Path>,
) {
    ui.horizontal(|ui| {
        for (tab, label_key) in [
            (
                FileHandlerSubTab::Detectors,
                "settings.file_handler.sub.detectors",
            ),
            (
                FileHandlerSubTab::Handlers,
                "settings.file_handler.sub.handlers",
            ),
            (
                FileHandlerSubTab::ExtensionMapping,
                "settings.file_handler.sub.extension_mapping",
            ),
            (
                FileHandlerSubTab::RecentPicks,
                "settings.file_handler.sub.recent_picks",
            ),
        ] {
            let selected = *sub_tab == tab;
            if ui.selectable_label(selected, t(label_key)).clicked() {
                *sub_tab = tab;
            }
        }
    });
    ui.separator();

    match *sub_tab {
        FileHandlerSubTab::ExtensionMapping => {
            draw_extension_mapping(ui, draft, new_ext_input, file_format)
        }
        FileHandlerSubTab::Detectors => draw_detectors(ui, fh_draft, file_format),
        FileHandlerSubTab::Handlers => {
            draw_handlers(ui, fh_draft, file_format, file_handler)
        }
        FileHandlerSubTab::RecentPicks => {
            draw_recent_picks(ui, recent_picks, recent_picks_path)
        }
    }
}

/// Sub-tab 상단 intro 블록: 본문 paragraph(wrap) + bullet 리스트.
fn draw_intro_block(ui: &mut egui::Ui, body_key: &str, bullet_keys: &[&str]) {
    ui.add_space(4.0);
    ui.add(egui::Label::new(t(body_key)).wrap());
    ui.add_space(4.0);
    for key in bullet_keys {
        ui.add(egui::Label::new(format!("• {}", t(key))).wrap());
    }
    ui.add_space(8.0);
}

/// Detectors sub-tab — Enabled 토글, user-origin 삭제, user 추가 form.
fn draw_detectors(
    ui: &mut egui::Ui,
    fh: &mut FileHandlerEditDraft,
    file_format: &FileFormatRegistry,
) {
    draw_intro_block(
        ui,
        "settings.file_handler.detectors.description",
        &[
            "settings.file_handler.detectors.bullet_what",
            "settings.file_handler.detectors.bullet_rules",
            "settings.file_handler.detectors.bullet_origin",
            "settings.file_handler.detectors.bullet_status",
            "settings.file_handler.detectors.bullet_add",
        ],
    );

    let ids = file_format.list_detectors();
    if ids.is_empty() {
        ui.label(t("settings.file_handler.detectors.empty"));
    } else {
        egui::Grid::new("file_handler_detectors_grid")
            .num_columns(5)
            .striped(true)
            .spacing(egui::vec2(12.0, 4.0))
            .show(ui, |ui| {
                ui.strong(t("settings.file_handler.detectors.col_status"));
                ui.strong(t("settings.file_handler.detectors.col_id"));
                ui.strong(t("settings.file_handler.detectors.col_origin"));
                ui.strong(t("settings.file_handler.detectors.col_rules"));
                ui.strong(""); // actions
                ui.end_row();

                for id in &ids {
                    let Some(det) = file_format.detector(id) else { continue };
                    // 효과 상태: draft 우선, 없으면 registry 상태.
                    let want_enabled = fh
                        .detector_enabled
                        .get(id)
                        .copied()
                        .unwrap_or(!det.disabled);
                    let mut checked = want_enabled;
                    if ui.checkbox(&mut checked, "").changed() {
                        fh.detector_enabled.insert(id.clone(), checked);
                    }
                    ui.label(id.as_str());
                    ui.label(detector_origins_summary(&det.rules));
                    ui.label(rule_kinds_summary(&det.rules));
                    // user-origin 항목이면 Remove 버튼.
                    let has_user = det
                        .rules
                        .iter()
                        .any(|r| matches!(r.origin, RuleOrigin::User));
                    let pending_remove = fh.remove_detector.contains(id);
                    if has_user {
                        ui.horizontal(|ui| {
                            if pending_remove {
                                ui.weak(t("settings.file_handler.common.pending_remove"));
                                if ui.small_button(t("button.cancel")).clicked() {
                                    fh.remove_detector.remove(id);
                                }
                            } else if ui
                                .small_button(t("settings.file_handler.detectors.remove_user"))
                                .clicked()
                            {
                                fh.remove_detector.insert(id.clone());
                            }
                        });
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(4.0);

    // "+ Add user detector" inline form.
    draw_add_detector_form(ui, fh);
}

fn draw_add_detector_form(ui: &mut egui::Ui, fh: &mut FileHandlerEditDraft) {
    let toggle_label = if fh.add_detector_form.open {
        t("settings.file_handler.detectors.add_close")
    } else {
        t("settings.file_handler.detectors.add_open")
    };
    if ui.button(toggle_label).clicked() {
        fh.add_detector_form.open = !fh.add_detector_form.open;
        fh.add_detector_form.error = None;
    }
    if !fh.add_detector_form.open {
        // 이미 추가된 draft 항목 표시.
        for decl in &fh.add_detector {
            ui.weak(format!(
                "{} {}",
                t("settings.file_handler.detectors.pending_add"),
                decl.id
            ));
        }
        return;
    }
    egui::Frame::new()
        .inner_margin(egui::vec2(8.0, 6.0))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .show(ui, |ui| {
            egui::Grid::new("file_handler_detectors_add_grid")
                .num_columns(2)
                .spacing(egui::vec2(8.0, 4.0))
                .show(ui, |ui| {
                    ui.label(t("settings.file_handler.detectors.field_id"));
                    ui.add(
                        egui::TextEdit::singleline(&mut fh.add_detector_form.id_input)
                            .hint_text("my-detector")
                            .desired_width(200.0),
                    );
                    ui.end_row();

                    ui.label(t("settings.file_handler.detectors.field_extensions"));
                    ui.add(
                        egui::TextEdit::singleline(&mut fh.add_detector_form.extensions_input)
                            .hint_text("md, mdx")
                            .desired_width(200.0),
                    );
                    ui.end_row();

                    ui.label(t("settings.file_handler.detectors.field_path_glob"));
                    ui.add(
                        egui::TextEdit::singleline(&mut fh.add_detector_form.path_glob_input)
                            .hint_text("Dockerfile")
                            .desired_width(200.0),
                    );
                    ui.end_row();
                });
            if let Some(err) = &fh.add_detector_form.error {
                ui.colored_label(egui::Color32::LIGHT_RED, err);
            }
            ui.horizontal(|ui| {
                if ui.button(t("button.add")).clicked() {
                    match build_add_detector_decl(&fh.add_detector_form) {
                        Ok(decl) => {
                            fh.add_detector.push(decl);
                            fh.add_detector_form = AddDetectorForm::default();
                        }
                        Err(e) => fh.add_detector_form.error = Some(e),
                    }
                }
                if ui.button(t("button.cancel")).clicked() {
                    fh.add_detector_form = AddDetectorForm::default();
                }
            });
        });
    for decl in &fh.add_detector {
        ui.weak(format!(
            "{} {}",
            t("settings.file_handler.detectors.pending_add"),
            decl.id
        ));
    }
}

fn build_add_detector_decl(form: &AddDetectorForm) -> Result<DetectorDecl, String> {
    let id = form.id_input.trim().to_string();
    if id.is_empty() {
        return Err(t("settings.file_handler.detectors.err_id_empty").to_string());
    }
    if !crate::file_format::is_valid_detector_id(&id) {
        return Err(t("settings.file_handler.detectors.err_id_invalid").to_string());
    }
    let mut rules = Vec::new();
    let exts: Vec<String> = form
        .extensions_input
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|s| {
            let v = s.trim().trim_start_matches('.').to_ascii_lowercase();
            (!v.is_empty()).then_some(v)
        })
        .collect();
    if !exts.is_empty() {
        rules.push(DetectorRuleDecl::Extension { values: exts });
    }
    let glob = form.path_glob_input.trim();
    if !glob.is_empty() {
        rules.push(DetectorRuleDecl::PathGlob {
            pattern: glob.to_string(),
        });
    }
    if rules.is_empty() {
        return Err(t("settings.file_handler.detectors.err_no_rules").to_string());
    }
    Ok(DetectorDecl {
        id,
        display_name_i18n_key: None,
        icon: None,
        disabled: false,
        rule: rules,
    })
}

/// Handlers sub-tab — Enabled 토글, user-origin 삭제, user 추가 form.
fn draw_handlers(
    ui: &mut egui::Ui,
    fh: &mut FileHandlerEditDraft,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
) {
    draw_intro_block(
        ui,
        "settings.file_handler.handlers.description",
        &[
            "settings.file_handler.handlers.bullet_what",
            "settings.file_handler.handlers.bullet_priority",
            "settings.file_handler.handlers.bullet_owner",
            "settings.file_handler.handlers.bullet_action_surface",
            "settings.file_handler.handlers.bullet_action_ipc",
            "settings.file_handler.handlers.bullet_action_system",
        ],
    );

    let ids = file_handler.list_handlers();
    if ids.is_empty() {
        ui.label(t("settings.file_handler.handlers.empty"));
    } else {
        let mut rows: Vec<crate::file_handler::FileHandler> = ids
            .iter()
            .filter_map(|id| file_handler.handler(id))
            .collect();
        rows.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });

        egui::Grid::new("file_handler_handlers_grid")
            .num_columns(7)
            .striped(true)
            .spacing(egui::vec2(12.0, 4.0))
            .show(ui, |ui| {
                ui.strong(t("settings.file_handler.handlers.col_status"));
                ui.strong(t("settings.file_handler.handlers.col_priority"));
                ui.strong(t("settings.file_handler.handlers.col_id"));
                ui.strong(t("settings.file_handler.handlers.col_owner"));
                ui.strong(t("settings.file_handler.handlers.col_detector"));
                ui.strong(t("settings.file_handler.handlers.col_action"));
                ui.strong("");
                ui.end_row();

                for h in &rows {
                    let want_enabled = fh
                        .handler_enabled
                        .get(&h.id)
                        .copied()
                        .unwrap_or(!h.disabled);
                    let mut checked = want_enabled;
                    if ui.checkbox(&mut checked, "").changed() {
                        fh.handler_enabled.insert(h.id.clone(), checked);
                    }
                    ui.label(h.priority.to_string());
                    ui.label(h.id.as_str());
                    ui.label(handler_owner_label(&h.owner));
                    ui.label(h.detector.as_str());
                    ui.label(handler_action_summary(&h.action));
                    if matches!(h.owner, HandlerOwner::User) {
                        let pending = fh.remove_handler.contains(&h.id);
                        ui.horizontal(|ui| {
                            if pending {
                                ui.weak(t("settings.file_handler.common.pending_remove"));
                                if ui.small_button(t("button.cancel")).clicked() {
                                    fh.remove_handler.remove(&h.id);
                                }
                            } else if ui
                                .small_button(t("settings.file_handler.handlers.remove_user"))
                                .clicked()
                            {
                                fh.remove_handler.insert(h.id.clone());
                            }
                        });
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(4.0);

    draw_add_handler_form(ui, fh, file_format);
}

fn draw_add_handler_form(
    ui: &mut egui::Ui,
    fh: &mut FileHandlerEditDraft,
    file_format: &FileFormatRegistry,
) {
    let toggle_label = if fh.add_handler_form.open {
        t("settings.file_handler.handlers.add_close")
    } else {
        t("settings.file_handler.handlers.add_open")
    };
    if ui.button(toggle_label).clicked() {
        fh.add_handler_form.open = !fh.add_handler_form.open;
        fh.add_handler_form.error = None;
    }
    if !fh.add_handler_form.open {
        for decl in &fh.add_handler {
            ui.weak(format!(
                "{} user/{}",
                t("settings.file_handler.handlers.pending_add"),
                decl.id.strip_prefix("user/").unwrap_or(&decl.id)
            ));
        }
        return;
    }
    let detector_ids = file_format.list_detectors();
    egui::Frame::new()
        .inner_margin(egui::vec2(8.0, 6.0))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .show(ui, |ui| {
            egui::Grid::new("file_handler_handlers_add_grid")
                .num_columns(2)
                .spacing(egui::vec2(8.0, 4.0))
                .show(ui, |ui| {
                    ui.label(t("settings.file_handler.handlers.field_short_name"));
                    ui.add(
                        egui::TextEdit::singleline(&mut fh.add_handler_form.short_name_input)
                            .hint_text("my-viewer")
                            .desired_width(200.0),
                    );
                    ui.end_row();

                    ui.label(t("settings.file_handler.handlers.field_detector"));
                    egui::ComboBox::from_id_salt("file_handler_handlers_add_detector")
                        .selected_text(if fh.add_handler_form.detector_id_input.is_empty() {
                            t("settings.file_handler.handlers.field_detector_select").to_string()
                        } else {
                            fh.add_handler_form.detector_id_input.clone()
                        })
                        .show_ui(ui, |ui| {
                            for id in &detector_ids {
                                ui.selectable_value(
                                    &mut fh.add_handler_form.detector_id_input,
                                    id.as_str().to_string(),
                                    id.as_str(),
                                );
                            }
                        });
                    ui.end_row();

                    ui.label(t("settings.file_handler.handlers.field_priority"));
                    ui.add(
                        egui::TextEdit::singleline(&mut fh.add_handler_form.priority_input)
                            .hint_text("100")
                            .desired_width(80.0),
                    );
                    ui.end_row();

                    ui.label(t("settings.file_handler.handlers.field_action_kind"));
                    egui::ComboBox::from_id_salt("file_handler_handlers_add_action_kind")
                        .selected_text(match fh.add_handler_form.action_kind {
                            AddHandlerActionKind::OpenSurface => {
                                t("settings.file_handler.handlers.action_open_surface").to_string()
                            }
                            AddHandlerActionKind::Ipc => {
                                t("settings.file_handler.handlers.action_ipc").to_string()
                            }
                            AddHandlerActionKind::System => {
                                t("settings.file_handler.handlers.action_system").to_string()
                            }
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut fh.add_handler_form.action_kind,
                                AddHandlerActionKind::OpenSurface,
                                t("settings.file_handler.handlers.action_open_surface"),
                            );
                            ui.selectable_value(
                                &mut fh.add_handler_form.action_kind,
                                AddHandlerActionKind::Ipc,
                                t("settings.file_handler.handlers.action_ipc"),
                            );
                            ui.selectable_value(
                                &mut fh.add_handler_form.action_kind,
                                AddHandlerActionKind::System,
                                t("settings.file_handler.handlers.action_system"),
                            );
                        });
                    ui.end_row();

                    match fh.add_handler_form.action_kind {
                        AddHandlerActionKind::OpenSurface => {
                            ui.label(t("settings.file_handler.handlers.field_surface_kind"));
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut fh.add_handler_form.action_surface_kind,
                                )
                                .hint_text("markdown_view")
                                .desired_width(200.0),
                            );
                            ui.end_row();
                            ui.label(t("settings.file_handler.handlers.field_param_key"));
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut fh.add_handler_form.action_param_key,
                                )
                                .hint_text("file")
                                .desired_width(120.0),
                            );
                            ui.end_row();
                        }
                        AddHandlerActionKind::Ipc => {
                            ui.label(t("settings.file_handler.handlers.field_ipc_method"));
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut fh.add_handler_form.action_ipc_method,
                                )
                                .hint_text("com.example.foo.open")
                                .desired_width(240.0),
                            );
                            ui.end_row();
                        }
                        AddHandlerActionKind::System => {}
                    }
                });
            if let Some(err) = &fh.add_handler_form.error {
                ui.colored_label(egui::Color32::LIGHT_RED, err);
            }
            ui.horizontal(|ui| {
                if ui.button(t("button.add")).clicked() {
                    match build_add_handler_decl(&fh.add_handler_form) {
                        Ok(decl) => {
                            fh.add_handler.push(decl);
                            fh.add_handler_form = AddHandlerForm::default();
                        }
                        Err(e) => fh.add_handler_form.error = Some(e),
                    }
                }
                if ui.button(t("button.cancel")).clicked() {
                    fh.add_handler_form = AddHandlerForm::default();
                }
            });
        });
    for decl in &fh.add_handler {
        ui.weak(format!(
            "{} {}",
            t("settings.file_handler.handlers.pending_add"),
            decl.id
        ));
    }
}

fn build_add_handler_decl(form: &AddHandlerForm) -> Result<UserHandlerUpsertDecl, String> {
    let short = form.short_name_input.trim();
    if short.is_empty() {
        return Err(t("settings.file_handler.handlers.err_short_name_empty").to_string());
    }
    if !crate::file_handler::is_valid_handler_short_name(short) {
        return Err(t("settings.file_handler.handlers.err_short_name_invalid").to_string());
    }
    let detector = form.detector_id_input.trim();
    if detector.is_empty() {
        return Err(t("settings.file_handler.handlers.err_detector_missing").to_string());
    }
    if !crate::file_format::is_valid_detector_id(detector) {
        return Err(t("settings.file_handler.handlers.err_detector_invalid").to_string());
    }
    let priority_str = form.priority_input.trim();
    let priority = if priority_str.is_empty() {
        100
    } else {
        priority_str
            .parse::<i32>()
            .map_err(|_| t("settings.file_handler.handlers.err_priority_invalid").to_string())?
    };
    let action = match form.action_kind {
        AddHandlerActionKind::OpenSurface => {
            let sk = form.action_surface_kind.trim();
            if sk.is_empty() {
                return Err(
                    t("settings.file_handler.handlers.err_surface_kind_empty").to_string()
                );
            }
            let pk = form.action_param_key.trim();
            UserHandlerActionDecl::OpenSurface {
                surface_kind: sk.to_string(),
                param_key: if pk.is_empty() { "file".to_string() } else { pk.to_string() },
            }
        }
        AddHandlerActionKind::Ipc => {
            let m = form.action_ipc_method.trim();
            if m.is_empty() {
                return Err(t("settings.file_handler.handlers.err_ipc_method_empty").to_string());
            }
            UserHandlerActionDecl::Ipc {
                method: m.to_string(),
            }
        }
        AddHandlerActionKind::System => UserHandlerActionDecl::System,
    };
    Ok(UserHandlerUpsertDecl {
        id: format!("user/{}", short),
        detector: Some(detector.to_string()),
        priority: Some(priority),
        display_name_i18n_key: None,
        disabled: None,
        action: Some(action),
    })
}

/// Recent picks sub-tab — `~/.tasty/file-handler-recent.json` 의 LRU 목록 + Forget.
fn draw_recent_picks(
    ui: &mut egui::Ui,
    recent_picks: &mut Option<RecentPicks>,
    path: Option<&std::path::Path>,
) {
    if recent_picks.is_none() {
        let picks = match path {
            Some(p) => RecentPicks::load(p),
            None => RecentPicks::default(),
        };
        *recent_picks = Some(picks);
    }
    let picks = recent_picks.as_mut().expect("loaded above");

    ui.add_space(4.0);
    ui.label(t("settings.file_handler.recent_picks.description"));
    ui.add_space(8.0);

    if picks.list().is_empty() {
        ui.label(t("settings.file_handler.recent_picks.empty"));
        return;
    }

    let entries: Vec<(crate::file_handler::HandlerId, i64)> = picks
        .list()
        .iter()
        .map(|e| (e.handler_id.clone(), e.last_used_at))
        .collect();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut to_forget: Option<crate::file_handler::HandlerId> = None;
    for (id, last_used) in &entries {
        ui.horizontal(|ui| {
            ui.label(id.as_str());
            ui.weak(format!("({})", format_relative_time(now_secs - last_used)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(t("settings.file_handler.recent_picks.forget"))
                    .clicked()
                {
                    to_forget = Some(id.clone());
                }
            });
        });
    }
    if let Some(id) = to_forget {
        if picks.forget(&id) {
            if let Some(p) = path {
                if let Err(e) = picks.save_atomic(p) {
                    tracing::warn!(
                        path = %p.display(),
                        error = %e,
                        "recent_picks: forget save failed",
                    );
                }
            }
        }
    }
}

fn detector_origins_summary(rules: &[crate::file_format::DetectorRule]) -> String {
    let mut origins: BTreeSet<String> = BTreeSet::new();
    for r in rules {
        let label = match &r.origin {
            RuleOrigin::HostDefault => "host".to_string(),
            RuleOrigin::Plugin(id) => format!("plugin:{}", id),
            RuleOrigin::User => "user".to_string(),
        };
        origins.insert(label);
    }
    if origins.is_empty() {
        "—".into()
    } else {
        origins.into_iter().collect::<Vec<_>>().join(", ")
    }
}

fn rule_kinds_summary(rules: &[crate::file_format::DetectorRule]) -> String {
    let mut kinds: Vec<&str> = rules
        .iter()
        .map(|r| match &r.kind {
            DetectorRuleKind::Extension { .. } => "ext",
            DetectorRuleKind::PathGlob { .. } => "glob",
            DetectorRuleKind::Mime { .. } => "mime",
            DetectorRuleKind::Magic { .. } => "magic",
            DetectorRuleKind::IsDirectory => "dir",
            DetectorRuleKind::Lua { .. } => "lua",
            DetectorRuleKind::StructureCheck { .. } => "structure",
            DetectorRuleKind::Unknown { .. } => "?",
        })
        .collect();
    kinds.sort();
    kinds.dedup();
    if kinds.is_empty() {
        "—".into()
    } else {
        kinds.join(", ")
    }
}

fn handler_owner_label(owner: &HandlerOwner) -> String {
    match owner {
        HandlerOwner::Host => "host".into(),
        HandlerOwner::Plugin(id) => format!("plugin:{}", id),
        HandlerOwner::User => "user".into(),
    }
}

fn handler_action_summary(action: &HandlerAction) -> String {
    match action {
        HandlerAction::OpenSurface { surface_kind, .. } => format!("surface:{}", surface_kind),
        HandlerAction::Ipc { method, .. } => format!("ipc:{}", method),
        HandlerAction::System => "system".into(),
    }
}

/// 상대 시간을 짧게 표시. UI 라벨이라 정밀도 < 가독성.
fn format_relative_time(secs_ago: i64) -> String {
    if secs_ago < 60 {
        return t("settings.file_handler.recent_picks.just_now").to_string();
    }
    if secs_ago < 3600 {
        return crate::i18n::t_fmt(
            "settings.file_handler.recent_picks.minutes_ago",
            &(secs_ago / 60).to_string(),
        );
    }
    if secs_ago < 86400 {
        return crate::i18n::t_fmt(
            "settings.file_handler.recent_picks.hours_ago",
            &(secs_ago / 3600).to_string(),
        );
    }
    crate::i18n::t_fmt(
        "settings.file_handler.recent_picks.days_ago",
        &(secs_ago / 86400).to_string(),
    )
}


/// Extension Mapping sub-tab. 광고된 모든 확장자 + draft 에 있는 확장자를 리스트로 표시,
/// 각 확장자 옆에 후보 detector 들을 ↑↓ 버튼으로 재정렬 가능.
fn draw_extension_mapping(
    ui: &mut egui::Ui,
    draft: &mut Option<BTreeMap<String, Vec<DetectorId>>>,
    new_ext_input: &mut String,
    file_format: &FileFormatRegistry,
) {
    // 초기 진입 시 registry 의 현재 priority 표를 draft 로 복사.
    if draft.is_none() {
        let mut map = BTreeMap::new();
        for ext in file_format.extension_priority_keys() {
            if let Some(order) = file_format.extension_priority_order(&ext) {
                map.insert(ext, order);
            }
        }
        *draft = Some(map);
    }
    let draft_map = draft.as_mut().expect("draft initialized above");

    draw_intro_block(
        ui,
        "settings.file_handler.extension_mapping.description",
        &[
            "settings.file_handler.extension_mapping.bullet_when",
            "settings.file_handler.extension_mapping.bullet_visibility",
            "settings.file_handler.extension_mapping.bullet_actions",
            "settings.file_handler.extension_mapping.bullet_unregistered",
        ],
    );

    // 표시할 확장자 = (draft 의 확장자) ∪ (모든 광고 확장자 중 1개 이상 candidate 가 있는 것).
    // candidate 가 2개 이상인 경우만 priority 의미가 있으므로 실제 노출은 후자 위주.
    let all_exts = file_format.all_advertised_extensions();
    let mut visible: std::collections::BTreeSet<String> =
        draft_map.keys().cloned().collect();
    for ext in &all_exts {
        let candidates = file_format.detectors_for_extension(ext);
        if candidates.len() >= 2 {
            visible.insert(ext.clone());
        }
    }

    if visible.is_empty() {
        ui.label(t("settings.file_handler.extension_mapping.no_conflicts"));
    } else {
        for ext in &visible {
            draw_extension_row(ui, ext, draft_map, file_format);
            ui.add_space(4.0);
        }
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(4.0);

    // 새 확장자 수동 추가 (자동완성 dropdown 대신 단순 textbox + suggestion 라벨).
    ui.horizontal(|ui| {
        ui.label(t("settings.file_handler.extension_mapping.add_label"));
        ui.add(
            egui::TextEdit::singleline(new_ext_input)
                .hint_text(".md")
                .desired_width(120.0),
        );
        let normalized = new_ext_input
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        let enabled = !normalized.is_empty()
            && !file_format.detectors_for_extension(&normalized).is_empty();
        if ui
            .add_enabled(enabled, egui::Button::new(t("button.add")))
            .clicked()
        {
            let order = file_format.detectors_for_extension(&normalized);
            if !order.is_empty() {
                draft_map.entry(normalized.clone()).or_insert(order);
            }
            new_ext_input.clear();
        }
    });
}

/// 한 확장자 행. 좌측에 확장자 라벨, 우측에 후보 detector 들의 ↑↓ 컨트롤.
fn draw_extension_row(
    ui: &mut egui::Ui,
    ext: &str,
    draft_map: &mut BTreeMap<String, Vec<DetectorId>>,
    file_format: &FileFormatRegistry,
) {
    let candidates = file_format.detectors_for_extension(ext);
    if candidates.is_empty() {
        egui::Frame::new()
            .inner_margin(egui::vec2(8.0, 6.0))
            .fill(ui.visuals().faint_bg_color)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(".{}", ext));
                    ui.label(t("settings.file_handler.extension_mapping.unregistered"));
                    if ui
                        .button(t("settings.file_handler.extension_mapping.clear"))
                        .clicked()
                    {
                        draft_map.insert(ext.to_string(), Vec::new());
                    }
                });
            });
        return;
    }

    let order: Vec<DetectorId> = if let Some(d) = draft_map.get(ext) {
        let mut result: Vec<DetectorId> = d.clone();
        for c in &candidates {
            if !result.contains(c) {
                result.push(c.clone());
            }
        }
        result
    } else {
        candidates.clone()
    };

    egui::Frame::new()
        .inner_margin(egui::vec2(8.0, 6.0))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!(".{}", ext));
                ui.add_space(8.0);
                if draft_map.contains_key(ext) {
                    if ui
                        .small_button(t("settings.file_handler.extension_mapping.reset"))
                        .on_hover_text(t(
                            "settings.file_handler.extension_mapping.reset_tooltip",
                        ))
                        .clicked()
                    {
                        draft_map.insert(ext.to_string(), Vec::new());
                    }
                }
            });
            ui.add_space(4.0);
            let len = order.len();
            let mut move_up: Option<usize> = None;
            let mut move_down: Option<usize> = None;
            for (i, id) in order.iter().enumerate() {
                let in_candidates = candidates.iter().any(|c| c == id);
                ui.horizontal(|ui| {
                    let up_enabled = i > 0 && in_candidates;
                    let down_enabled = i + 1 < len && in_candidates;
                    if ui
                        .add_enabled(up_enabled, egui::Button::new("▲"))
                        .clicked()
                    {
                        move_up = Some(i);
                    }
                    if ui
                        .add_enabled(down_enabled, egui::Button::new("▼"))
                        .clicked()
                    {
                        move_down = Some(i);
                    }
                    if in_candidates {
                        ui.label(id.as_str());
                        if !file_format.is_enabled(id) {
                            ui.weak(format!(
                                "({})",
                                t("settings.file_handler.extension_mapping.disabled")
                            ));
                        }
                    } else {
                        ui.weak(format!(
                            "{} ({})",
                            id.as_str(),
                            t("settings.file_handler.extension_mapping.unregistered")
                        ));
                    }
                });
            }
            if let Some(i) = move_up {
                let mut new_order = order.clone();
                new_order.swap(i - 1, i);
                draft_map.insert(ext.to_string(), new_order);
            } else if let Some(i) = move_down {
                let mut new_order = order.clone();
                new_order.swap(i, i + 1);
                draft_map.insert(ext.to_string(), new_order);
            }
        });
}
