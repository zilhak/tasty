//! mirror(원격 attach) origin의 파일 열기. 경로는 원격 파일이라 client 로컬에서 식별·실행하지 않는다.
//! 식별은 파일 이름만 보는 DetectDepth::Name으로 끝나며, 원격 NewTab으로 보낼 수 있는 핸들러만 실행한다.
//! 규칙은 [ADR-0022](../../../docs/adr/0022-remote-mirror-content-and-queries.md)를 따른다.

use crate::app::services::AppServices;
use crate::core::State;
use crate::file::dispatch::{DispatchTarget, FileDispatchOrigin};
use crate::file::format::{DetectorId, FileTarget};
use crate::file::handler::{FileHandler, HandlerAction};
use crate::runtime::engine_access::EngineMut;
use crate::state::RequestContext;

/// 원격에 열 수 있는 핸들러인지 확인한다.
/// OpenSurface이면서 client가 그 kind의 콘텐츠를 mirror하는 경우만 대상이다.
/// System·Ipc는 원격 경로를 client의 OS나 plugin이 로컬 파일로 열게 되어 제외한다.
pub(crate) fn is_remote_openable(engine: &CoreState, handler: &FileHandler) -> bool {
    match &handler.action {
        HandlerAction::OpenSurface { surface_kind, .. } => mirrors_content(engine, surface_kind),
        HandlerAction::Ipc { .. } | HandlerAction::System => false,
    }
}

/// mirror origin의 경로는 원격 파일이다. picker 선택 등 어느 경로로 와도 원격에 열 수 없는 핸들러는
/// 실행하지 않는다. 막았으면 true다.
pub(crate) fn rejects_handler_for_origin(
    engine: &CoreState,
    handler: &FileHandler,
    origin_surface_id: Option<u32>,
) -> bool {
    let rejected = origin_surface_id.is_some_and(|sid| engine.is_mirror_surface(sid))
        && !is_remote_openable(engine, handler);
    if rejected {
        tracing::warn!(
            handler_id = %handler.id,
            "file handler cannot open a remote file; not executed",
        );
    }
    rejected
}

/// kind와 client에 등록한 plugin 쌍이 원문 전달이나 mesh mirror 허용 목록에 있는지 확인한다.
/// html처럼 mirror에서 placeholder로 보이는 kind는 원격에 열어도 내용을 볼 수 없다.
fn mirrors_content(engine: &CoreState, kind: &str) -> bool {
    let Some(def) = engine.runtime.surface_registry.get_live(kind) else {
        return false;
    };
    let Some(plugin_id) = def.source.plugin_id() else {
        return false;
    };
    crate::remote::server::is_attach_content_allowed(kind, plugin_id)
        || crate::runtime::surface_registry::egui_mesh::is_egui_mesh_allowed(kind, plugin_id)
}

/// mirror origin의 식별 결과를 적용한다. 로컬 핸들러 전체 picker는 띄우지 않는다.
/// 1순위가 원격에 열 수 있으면 바로 실행한다. 아니면 사용자 요청과 사용자 입력을 증명하지 못한 plugin 중계 요청에는
/// 원격에 열 수 있는 핸들러만 담은 picker를 띄우고, 외부 IPC 요청은 사용자 화면에 팝업을 만들지 않도록
/// 그중 첫 핸들러를 실행한다.
pub(crate) fn apply_remote_identify_result(
    core: &mut AppServices,
    state: &mut RequestContext,
    engine: &mut EngineMut<'_>,
    target: FileTarget,
    detector: Option<DetectorId>,
    origin_surface_id: u32,
    dispatch_origin: FileDispatchOrigin,
) {
    let handlers = detector
        .as_ref()
        .map(|d| engine.runtime.file_handler.handlers_for(d))
        .unwrap_or_default();
    let first_is_openable = handlers
        .first()
        .is_some_and(|h| is_remote_openable(engine, h));
    let openable: Vec<FileHandler> = handlers
        .into_iter()
        .filter(|h| is_remote_openable(engine, h))
        .collect();
    let Some(first) = openable.first() else {
        report_unsupported(state, &target, dispatch_origin);
        return;
    };
    if first_is_openable || dispatch_origin == FileDispatchOrigin::Agent {
        let first = first.clone();
        crate::file::dispatch::execute_handler_action(
            core,
            state,
            engine,
            &first,
            &DispatchTarget::File(target),
            Some(origin_surface_id),
            dispatch_origin,
            false,
        );
        return;
    }
    open_remote_picker(
        state,
        engine,
        target,
        detector,
        openable,
        origin_surface_id,
        dispatch_origin,
    );
}

/// 기존 핸들러 picker를 원격에 열 수 있는 후보만으로 연다. 최근 목록도 같은 조건으로 거른다.
/// 1순위는 원격에 열 수 없어 목록에 없으므로 기본 핸들러 표시는 두지 않는다.
fn open_remote_picker(
    state: &mut RequestContext,
    engine: &mut CoreState,
    target: FileTarget,
    detector: Option<DetectorId>,
    openable: Vec<FileHandler>,
    origin_surface_id: u32,
    dispatch_origin: FileDispatchOrigin,
) {
    crate::file::dispatch::open_picker(
        state,
        engine,
        DispatchTarget::File(target),
        detector,
        openable,
        false,
        dispatch_origin,
        false,
    );
    let Some(picker) = state.dialogs.file_handler_picker.as_mut() else {
        return;
    };
    picker.origin_surface_id = Some(origin_surface_id);
    picker.default_handler = None;
    picker.recent.retain(|summary| {
        engine
            .runtime
            .file_handler
            .get(&summary.id)
            .is_some_and(|h| is_remote_openable(engine, &h))
    });
}

fn report_unsupported(state: &mut RequestContext, target: &FileTarget, origin: FileDispatchOrigin) {
    match origin {
        FileDispatchOrigin::User | FileDispatchOrigin::PluginUnverified => state.toasts.push(
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
