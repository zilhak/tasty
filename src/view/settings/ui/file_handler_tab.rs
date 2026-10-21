//! Handler 설정: detector, handler, 확장자 우선순위와 훅 핸들러를 편집한다.
//! 변경은 초안에 보관하고 Save에서 레지스트리와 사용자 TOML에 반영한다.

use crate::runtime::file_catalog::FormatCatalog as FileFormatRegistry;
use crate::runtime::file_catalog::HandlerCatalog as FileHandlerRegistry;
use std::collections::{BTreeMap, BTreeSet};

use crate::file::format::{DetectorDecl, DetectorId};
use crate::file::handler::{HandlerId, UserHandlerUpsertDecl};
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
    /// 추가·교체를 먼저, 켜기 상태를 다음에, 삭제를 마지막에 적용한다. 추가는 같은 출처의
    /// 기여를 통째로 교체하므로 먼저 해야 같은 초안의 켜기 변경이 남는다. 삭제는 사용자 기여
    /// 전체를 지우므로 마지막이다.
    pub(crate) fn into_edits(self) -> Vec<crate::app::settings_edit::RegistryEdit> {
        use crate::app::settings_edit::RegistryEdit as E;
        self.add_detector
            .into_iter()
            .map(E::AddDetector)
            .chain(self.add_handler.into_iter().map(E::AddHandler))
            .chain(
                self.detector_enabled
                    .into_iter()
                    .map(|(id, value)| E::DetectorEnabled(id, value)),
            )
            .chain(
                self.handler_enabled
                    .into_iter()
                    .map(|(id, value)| E::HandlerEnabled(id, value)),
            )
            .chain(self.remove_detector.into_iter().map(E::RemoveDetector))
            .chain(self.remove_handler.into_iter().map(E::RemoveHandler))
            .collect()
    }
}

/// detector 추가 카드의 입력 상태.
#[derive(Debug, Clone, Default)]
struct AddDetectorForm {
    open: bool,
    id_input: String,
    /// 콤마 또는 공백 구분의 확장자 목록.
    extensions_input: String,
    /// 단일 path-glob 패턴 (옵션).
    path_glob_input: String,
    /// Add를 한 번 눌렀는지. 형식 오류는 그 뒤부터 보인다.
    tried: bool,
}

/// handler 추가 카드의 입력 상태.
#[derive(Debug, Clone, Default)]
struct AddHandlerForm {
    open: bool,
    short_name_input: String,
    /// 고른 detector id. 목록에서 사라지면 비운다.
    detector: Option<String>,
    priority_input: String, // 비우면 default 100
    action_kind: AddHandlerActionKind,
    action_surface_kind: String,
    action_param_key: String,
    action_ipc_method: String,
    tried: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AddHandlerActionKind {
    #[default]
    OpenSurface,
    Ipc,
    System,
}

impl AddHandlerActionKind {
    const ALL: [Self; 3] = [Self::OpenSurface, Self::Ipc, Self::System];

    fn label_key(self) -> &'static str {
        match self {
            Self::OpenSurface => "settings.file_handler.handlers.action_open_surface",
            Self::Ipc => "settings.file_handler.handlers.action_ipc",
            Self::System => "settings.file_handler.handlers.action_system",
        }
    }
}

/// FileHandler 탭 콘텐츠. L2 사이드바(섹션 목록·필터·선택)는 settings 셸이
/// 소유하므로 여기서는 활성 `sub_tab` 의 콘텐츠만 그린다.
// 설정 창이 각 초안을 따로 소유하므로 참조를 각각 전달받는다.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_file_handler_tab(
    ui: &mut egui::Ui,
    sub_tab: FileHandlerSubTab,
    draft: &mut Option<BTreeMap<String, Vec<DetectorId>>>,
    pending: &mut BTreeMap<String, Option<Vec<DetectorId>>>,
    new_ext_input: &mut String,
    fh_draft: &mut FileHandlerEditDraft,
    hh_draft: &mut HookHandlerEditDraft,
    keybindings: &crate::settings::KeybindingSettings,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
) {
    match sub_tab {
        FileHandlerSubTab::ExtensionMapping => {
            draw_extension_mapping(ui, draft, pending, new_ext_input, file_format)
        }
        FileHandlerSubTab::Detectors => draw_detectors(ui, fh_draft, file_format),
        FileHandlerSubTab::Handlers => draw_handlers(ui, fh_draft, file_format, file_handler),
        FileHandlerSubTab::HookHandlers => draw_hook_handlers(ui, hh_draft, keybindings),
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

/// 서브탭 머리 — 설명과 출처 불릿 세 줄, 오른쪽 Add 버튼. 눌렸으면 true.
pub(super) fn draw_fh_intro(
    ui: &mut egui::Ui,
    body_key: &str,
    add_key: &str,
    adding: bool,
) -> bool {
    let th = crate::theme::theme();
    let host = t("settings.file_handler.common.origin_host");
    let plugin = t("settings.file_handler.common.origin_plugin");
    let user = t("settings.file_handler.common.origin_user");
    tasty_ui_widgets::fh_intro(
        ui,
        &th,
        t(body_key),
        &[("host", host), ("plugin:<id>", plugin), ("user", user)],
        t(add_key),
        adding,
    )
}

/// 행이 공유하는 고정 문구.
pub(super) struct RowLabels {
    undo: &'static str,
    remove: &'static str,
}

impl RowLabels {
    pub(super) fn get() -> Self {
        Self {
            undo: t("settings.file_handler.common.undo"),
            remove: t("settings.file_handler.common.remove"),
        }
    }
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
