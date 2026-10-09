//! debug 빌드의 설정 창 열기·탭 전환·닫기를 처리한다.
//! 설정 창은 별도 winit 창이라 App 이벤트로 열고, 이미 열린 창은 그 창의 상태를 바로 바꾼다.

use winit::window::WindowId;

use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};
use crate::settings_ui::DebugTabsApplied;
use crate::view::{ActiveModal, ModalKind};

/// 설정 창 debug 요청이 다룰 대상. 활성 모달의 종류만 본다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTarget {
    /// 설정 창이 열려 있다.
    Open(WindowId),
    /// 열린 모달이 없다.
    NoModal,
    /// 다른 모달이 열려 있다. 모달은 한 번에 하나라 설정 창을 열 수 없다.
    Other(ModalKind),
}

fn settings_target(active: Option<ActiveModal>) -> SettingsTarget {
    match active {
        None => SettingsTarget::NoModal,
        Some(m) if m.kind == ModalKind::Settings => SettingsTarget::Open(m.id),
        Some(m) => SettingsTarget::Other(m.kind),
    }
}

/// 알 수 없는 탭 키는 요청을 거절하지 않고 경고만 남긴다.
pub(crate) fn warn_unknown_debug_tabs(
    tab: Option<&str>,
    subtab: Option<&str>,
    applied: DebugTabsApplied,
) {
    if let (Some(key), Some(false)) = (tab, applied.tab) {
        tracing::warn!("debug.settings.open: unknown settings tab '{key}'");
    }
    if let (Some(key), Some(false)) = (subtab, applied.subtab) {
        tracing::warn!("debug.settings.open: unknown settings subtab '{key}'");
    }
}

fn read_str(params: &serde_json::Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

impl App {
    /// 열린 설정 창이 있으면 그 창의 탭을 바꾸고, 없으면 열기를 예약한다.
    /// 다른 모달이 열려 있으면 예약하지 않고 거절한다 — 예약이 남으면 다음 열기에 섞인다.
    pub(super) fn ipc_handle_debug_settings_open(&mut self, cmd: &IpcCommand) -> IpcStep {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let tab = read_str(&cmd.request.params, "tab");
        let subtab = read_str(&cmd.request.params, "subtab");
        let response = match settings_target(self.view.active_modal()) {
            SettingsTarget::Open(window_id) => {
                match self
                    .view
                    .views
                    .get_mut(&window_id)
                    .and_then(|v| v.as_any_mut().downcast_mut::<crate::view::SettingsView>())
                {
                    Some(modal) => {
                        let applied = modal.apply_debug_tabs(tab.as_deref(), subtab.as_deref());
                        warn_unknown_debug_tabs(tab.as_deref(), subtab.as_deref(), applied);
                        host_ipc::protocol::JsonRpcResponse::success(
                            id,
                            serde_json::json!({
                                "scheduled": false,
                                "window_id": u64::from(window_id),
                                "tab": tab,
                                "subtab": subtab,
                                "tab_applied": applied.tab,
                                "subtab_applied": applied.subtab,
                            }),
                        )
                    }
                    None => host_ipc::protocol::JsonRpcResponse::error(
                        id,
                        -32603,
                        "Active settings modal is not in the view registry",
                    ),
                }
            }
            SettingsTarget::NoModal => {
                self.state.pending_settings_tab = tab.clone();
                self.state.pending_settings_subtab = subtab.clone();
                crate::shortcuts::send_app_event(&self.view.proxy, crate::AppEvent::OpenSettings);
                host_ipc::protocol::JsonRpcResponse::success(
                    id,
                    serde_json::json!({ "scheduled": true, "tab": tab, "subtab": subtab }),
                )
            }
            SettingsTarget::Other(kind) => host_ipc::protocol::JsonRpcResponse::error(
                id,
                -32000,
                format!(
                    "Another modal is open ({}); close it first with 'debug.modal.close_request'",
                    kind.as_str()
                ),
            ),
        };
        send_response(&cmd.response_tx, response);
        IpcStep::Handled
    }

    /// 설정 창만 닫는다. 바닥글 저장을 누르지 않았으므로 편집 내용은 저장하지 않는다(취소와 같다).
    /// 다른 모달은 닫지 않는다.
    pub(super) fn ipc_handle_debug_settings_close(&mut self, cmd: &IpcCommand) -> IpcStep {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let result = match settings_target(self.view.active_modal()) {
            SettingsTarget::Open(window_id) => {
                self.close_active_modal();
                serde_json::json!({ "closed": true, "window_id": u64::from(window_id) })
            }
            SettingsTarget::NoModal => serde_json::json!({ "closed": false }),
            SettingsTarget::Other(kind) => {
                serde_json::json!({ "closed": false, "active_modal_kind": kind.as_str() })
            }
        };
        send_response(
            &cmd.response_tx,
            host_ipc::protocol::JsonRpcResponse::success(id, result),
        );
        IpcStep::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modal(id: u64, kind: ModalKind) -> Option<ActiveModal> {
        Some(ActiveModal {
            id: WindowId::from(id),
            kind,
        })
    }

    /// 열린 설정 창은 그 창 ID로, 모달이 없으면 열기 예약으로, 다른 모달이면 거절로 간다.
    #[test]
    fn the_active_modal_decides_switch_schedule_or_refuse() {
        assert_eq!(
            settings_target(modal(7, ModalKind::Settings)),
            SettingsTarget::Open(WindowId::from(7u64))
        );
        assert_eq!(settings_target(None), SettingsTarget::NoModal);
        assert_eq!(
            settings_target(modal(8, ModalKind::Plugins)),
            SettingsTarget::Other(ModalKind::Plugins)
        );
        assert_eq!(
            settings_target(modal(9, ModalKind::Quit)),
            SettingsTarget::Other(ModalKind::Quit)
        );
    }
}
