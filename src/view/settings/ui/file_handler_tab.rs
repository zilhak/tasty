//! Handler 설정: detector, handler, 확장자 우선순위와 훅 핸들러를 편집한다.
//! 변경은 초안에 보관하고 Save에서 레지스트리와 사용자 TOML에 반영한다.

use std::collections::{BTreeMap, BTreeSet};

use crate::file::format::{DetectorDecl, DetectorId, FileFormatRegistry};
use crate::file::handler::{FileHandlerRegistry, HandlerId, UserHandlerUpsertDecl};
use crate::i18n::t;

/// 파일 라우팅 설정 세 종류와 훅 핸들러 설정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileHandlerSubTab {
    Detectors,
    Handlers,
    ExtensionMapping,
    HookHandlers,
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
        apply_detector_changes(
            &self.detector_enabled,
            &self.remove_detector,
            self.add_detector,
            file_format,
        );
        apply_handler_changes(
            &self.handler_enabled,
            &self.remove_handler,
            self.add_handler,
            file_handler,
        );
    }
}

/// detector 관련 draft(enabled 토글/삭제/추가)를 registry 에 commit.
fn apply_detector_changes(
    detector_enabled: &BTreeMap<DetectorId, bool>,
    remove_detector: &BTreeSet<DetectorId>,
    add_detector: Vec<DetectorDecl>,
    file_format: &FileFormatRegistry,
) {
    for (id, enabled) in detector_enabled {
        // 사용자 요청을 명시적인 override로 반영한다.
        file_format.set_user_detector_disabled(id, !enabled);
    }
    for id in remove_detector {
        file_format.remove_user_detector(id);
    }
    for decl in add_detector {
        if let Err(e) = file_format.upsert_user_detector(decl) {
            tracing::warn!("file_handler tab: upsert_user_detector failed: {e}");
        }
    }
}

/// handler 관련 draft(enabled 토글/삭제/추가)를 registry 에 commit.
fn apply_handler_changes(
    handler_enabled: &BTreeMap<HandlerId, bool>,
    remove_handler: &BTreeSet<HandlerId>,
    add_handler: Vec<UserHandlerUpsertDecl>,
    file_handler: &FileHandlerRegistry,
) {
    for (id, enabled) in handler_enabled {
        file_handler.set_user_handler_disabled(id, !enabled);
    }
    for id in remove_handler {
        file_handler.remove_user_handler(id);
    }
    for decl in add_handler {
        if let Err(e) = file_handler.upsert_user_handler(decl) {
            tracing::warn!("file_handler tab: upsert_user_handler failed: {e}");
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

/// FileHandler 탭 콘텐츠. L2 사이드바(섹션 목록·필터·선택)는 settings 셸이
/// 소유하므로 여기서는 활성 `sub_tab` 의 콘텐츠만 그린다.
// 설정 창이 각 초안을 따로 소유하므로 참조를 각각 전달받는다.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_file_handler_tab(
    ui: &mut egui::Ui,
    sub_tab: FileHandlerSubTab,
    draft: &mut Option<BTreeMap<String, Vec<DetectorId>>>,
    new_ext_input: &mut String,
    fh_draft: &mut FileHandlerEditDraft,
    hh_draft: &mut HookHandlerEditDraft,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
) {
    match sub_tab {
        FileHandlerSubTab::ExtensionMapping => {
            draw_extension_mapping(ui, draft, new_ext_input, file_format)
        }
        FileHandlerSubTab::Detectors => draw_detectors(ui, fh_draft, file_format),
        FileHandlerSubTab::Handlers => draw_handlers(ui, fh_draft, file_format, file_handler),
        FileHandlerSubTab::HookHandlers => draw_hook_handlers(ui, hh_draft),
    }
}

/// Sub-tab 상단 intro 블록: 본문 paragraph(wrap) + bullet 리스트.
pub(super) fn draw_intro_block(ui: &mut egui::Ui, body_key: &str, bullet_keys: &[&str]) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_xs);
    ui.add(egui::Label::new(t(body_key)).wrap());
    vspace(ui, th.spacing_xs);
    for key in bullet_keys {
        ui.add(egui::Label::new(format!("• {}", t(key))).wrap());
    }
    vspace(ui, th.spacing_sm);
}

mod detectors;
mod extension_mapping;
mod handlers;
mod hook_handlers;

use detectors::draw_detectors;
use extension_mapping::draw_extension_mapping;
use handlers::draw_handlers;
pub(crate) use hook_handlers::HookHandlerEditDraft;
use hook_handlers::draw_hook_handlers;
use tasty_ui_widgets::vspace;
