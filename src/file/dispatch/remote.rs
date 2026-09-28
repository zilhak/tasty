//! mirror(원격 attach) origin의 파일 열기. 경로는 원격 파일이라 client 로컬에서 식별·실행하지 않는다.
//! 식별은 파일 이름만 보는 DetectDepth::Name으로 끝나며, 원격 NewTab으로 보낼 수 있는 핸들러만 실행한다.
//! 규칙은 [ADR-0022](../../../docs/adr/0022-remote-mirror-content-and-queries.md)를 따른다.

use crate::core::{Core, CoreState};
use crate::file::dispatch::{DispatchTarget, FileDispatchOrigin};
use crate::file::format::{DetectorId, FileTarget};
use crate::file::handler::{FileHandler, HandlerAction};
use crate::state::AppState;

/// 원격에 열 수 있는 첫 핸들러를 우선순위 순서로 고른다.
/// OpenSurface이면서 client가 그 kind의 콘텐츠를 mirror하는 경우만 대상이다.
/// System·Ipc는 원격 경로를 client의 OS나 plugin이 로컬 파일로 열게 되어 제외한다.
pub(crate) fn remote_openable_handler(
    engine: &CoreState,
    handlers: Vec<FileHandler>,
) -> Option<FileHandler> {
    handlers.into_iter().find(|h| match &h.action {
        HandlerAction::OpenSurface { surface_kind, .. } => mirrors_content(engine, surface_kind),
        HandlerAction::Ipc { .. } | HandlerAction::System => false,
    })
}

/// kind와 client에 등록한 plugin 쌍이 원문 전달이나 mesh mirror 허용 목록에 있는지 확인한다.
/// html처럼 mirror에서 placeholder로 보이는 kind는 원격에 열어도 내용을 볼 수 없다.
fn mirrors_content(engine: &CoreState, kind: &str) -> bool {
    let Some(def) = engine.surface_registry.get_live(kind) else {
        return false;
    };
    let Some(plugin_id) = def.source.plugin_id() else {
        return false;
    };
    crate::core::attach_runtime::is_attach_content_allowed(kind, plugin_id)
        || crate::core::surface_registry::egui_mesh::is_egui_mesh_allowed(kind, plugin_id)
}

/// mirror origin의 식별 결과를 적용한다. 로컬 핸들러 전체 picker는 띄우지 않는다.
pub(crate) fn apply_remote_identify_result(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    target: FileTarget,
    detector: Option<DetectorId>,
    origin_surface_id: u32,
    dispatch_origin: FileDispatchOrigin,
) {
    let handlers = detector
        .as_ref()
        .map(|d| engine.file_handler.handlers_for(d))
        .unwrap_or_default();
    let Some(handler) = remote_openable_handler(engine, handlers) else {
        report_unsupported(state, &target, dispatch_origin);
        return;
    };
    crate::file::dispatch::execute_handler_action(
        core,
        state,
        engine,
        &handler,
        &DispatchTarget::File(target),
        Some(origin_surface_id),
        dispatch_origin,
        false,
    );
}

fn report_unsupported(state: &mut AppState, target: &FileTarget, origin: FileDispatchOrigin) {
    match origin {
        FileDispatchOrigin::User => state.toasts.push(
            crate::i18n::t("explorer.state.remote_open_unsupported").to_string(),
            crate::adapters::ui::ToastKind::Info,
            crate::adapters::ui::ToastScope::Window,
        ),
        FileDispatchOrigin::Agent => tracing::warn!(
            target = %target.display(),
            "no file handler can open this remote file; not executed",
        ),
    }
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
