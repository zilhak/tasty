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

use crate::file::format::{DetectorDecl, DetectorId, FileFormatRegistry};
use crate::file::handler::{FileHandlerRegistry, HandlerId, UserHandlerUpsertDecl};
use crate::i18n::t;

/// FileHandler 탭의 sub-tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileHandlerSubTab {
    Detectors,
    Handlers,
    ExtensionMapping,
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

    pub fn apply(self, file_format: &FileFormatRegistry, file_handler: &FileHandlerRegistry) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AddHandlerActionKind {
    #[default]
    OpenSurface,
    Ipc,
    System,
}

pub(crate) fn draw_file_handler_tab(
    ui: &mut egui::Ui,
    sub_tab: &mut FileHandlerSubTab,
    draft: &mut Option<BTreeMap<String, Vec<DetectorId>>>,
    new_ext_input: &mut String,
    fh_draft: &mut FileHandlerEditDraft,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
    l2_filter: &mut String,
) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    let sections: Vec<(FileHandlerSubTab, String)> = vec![
        (
            FileHandlerSubTab::ExtensionMapping,
            t("settings.file_handler.sub.extension_mapping").to_string(),
        ),
        (
            FileHandlerSubTab::Detectors,
            t("settings.file_handler.sub.detectors").to_string(),
        ),
        (
            FileHandlerSubTab::Handlers,
            t("settings.file_handler.sub.handlers").to_string(),
        ),
    ];

    let current = *sub_tab;
    let mut selected_new: Option<FileHandlerSubTab> = None;
    let filter_lc = l2_filter.to_lowercase();
    tasty_ui_widgets::two_depth_layout_filtered(
        ui,
        &th,
        available_height,
        l2_filter,
        t("settings.filter.sections"),
        |ui| {
            let mut any = false;
            for (tab, label) in &sections {
                if !filter_lc.is_empty() && !label.to_lowercase().contains(&filter_lc) {
                    continue;
                }
                any = true;
                let selected = current == *tab;
                if ui.selectable_label(selected, label.as_str()).clicked() {
                    selected_new = Some(*tab);
                }
            }
            if !any {
                ui.label(egui::RichText::new(t("settings.filter.no_matches")).color(th.subtext0));
            }
        },
        |ui| match current {
            FileHandlerSubTab::ExtensionMapping => {
                draw_extension_mapping(ui, draft, new_ext_input, file_format)
            }
            FileHandlerSubTab::Detectors => draw_detectors(ui, fh_draft, file_format),
            FileHandlerSubTab::Handlers => draw_handlers(ui, fh_draft, file_format, file_handler),
        },
    );
    if let Some(new) = selected_new {
        *sub_tab = new;
    }
}

/// Sub-tab 상단 intro 블록: 본문 paragraph(wrap) + bullet 리스트.
pub(super) fn draw_intro_block(ui: &mut egui::Ui, body_key: &str, bullet_keys: &[&str]) {
    ui.add_space(4.0);
    ui.add(egui::Label::new(t(body_key)).wrap());
    ui.add_space(4.0);
    for key in bullet_keys {
        ui.add(egui::Label::new(format!("• {}", t(key))).wrap());
    }
    ui.add_space(8.0);
}

mod detectors;
mod extension_mapping;
mod handlers;

use detectors::draw_detectors;
use extension_mapping::draw_extension_mapping;
use handlers::draw_handlers;
