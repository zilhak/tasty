//! Popup 인스턴스 생명주기: open/event/close, host 발급 instance_id 관리.
//! `send_surface_request` 헬퍼와 `send_command_invoke` 단축키 dispatch 도 포함.

use std::sync::atomic::Ordering;

use serde_json::json;

use crate::protocol::{self, PluginRequest};

use super::{PendingRequestKind, PluginManager, PopupInstance};

impl PluginManager {
    pub(super) fn send_surface_request(
        &mut self,
        plugin_id: &str,
        method: &str,
        params: serde_json::Value,
        kind: PendingRequestKind,
    ) {
        let proc = match self.processes.get(plugin_id) {
            Some(p) => p,
            None => return,
        };
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let req = PluginRequest {
            method: method.to_string(),
            params,
            id,
        };
        if proc.req_tx.send(req).is_ok() {
            self.pending_requests.insert(id, kind);
        }
    }

    /// Plugin이 contribute한 popup의 새 인스턴스를 연다. plugin process에
    /// `popup.open` IPC를 보내고, 응답으로 받은 초기 tree를 popup_instances에 저장.
    ///
    /// 반환값은 호스트가 발급한 instance_id. 추후 `send_popup_event` / `close_popup`에
    /// 같은 id를 넘겨야 한다.
    ///
    /// plugin이 실행 중이 아니거나 popup contribute가 없으면 `None`.
    pub fn open_popup_instance(
        &mut self,
        plugin_id: &str,
        popup_id: &str,
        context: serde_json::Value,
    ) -> Option<u64> {
        if !self.processes.contains_key(plugin_id) {
            tracing::warn!(
                "popup.open: plugin '{plugin_id}' is not running, dropping popup '{popup_id}'"
            );
            return None;
        }
        let pkg = self.packages.iter().find(|p| p.manifest.id == plugin_id)?;
        let contribute = pkg
            .manifest
            .contributes
            .popup
            .iter()
            .find(|p| p.id == popup_id)
            .cloned()?;
        let instance_id = self.next_popup_instance_id;
        self.next_popup_instance_id = self.next_popup_instance_id.wrapping_add(1);
        self.popup_instances.insert(
            instance_id,
            PopupInstance {
                plugin_id: plugin_id.to_string(),
                popup_id: popup_id.to_string(),
                contribute,
                tree: None,
            },
        );
        self.send_surface_request(
            plugin_id,
            protocol::METHOD_POPUP_OPEN,
            json!({
                "popup_id": popup_id,
                "instance_id": instance_id,
                "context": context,
            }),
            PendingRequestKind::PopupOpen { instance_id },
        );
        Some(instance_id)
    }

    /// popup 인스턴스 위에서 발생한 사용자 이벤트를 plugin에 forward.
    /// plugin 응답은 [`PopupEventResult`] — 갱신된 tree와 자체 닫기 요청 플래그.
    pub fn send_popup_event(&mut self, instance_id: u64, event: &tasty_plugin_protocol::UiEvent) {
        let plugin_id = match self.popup_instances.get(&instance_id) {
            Some(inst) => inst.plugin_id.clone(),
            None => {
                tracing::warn!("popup.event: unknown instance_id {instance_id}");
                return;
            }
        };
        let event_value = match serde_json::to_value(event) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("popup.event: failed to serialize event: {e}");
                return;
            }
        };
        self.send_surface_request(
            &plugin_id,
            protocol::METHOD_POPUP_EVENT,
            json!({
                "instance_id": instance_id,
                "event": event_value,
            }),
            PendingRequestKind::PopupEvent { instance_id },
        );
    }

    /// popup 인스턴스를 닫는다. plugin에 `popup.closed` fire-and-forget IPC를 보내고
    /// 호스트 측 인스턴스도 제거. 닫는 이유는 [`PopupCloseReason`] 그대로 전달.
    pub fn close_popup_instance(
        &mut self,
        instance_id: u64,
        reason: tasty_plugin_protocol::PopupCloseReason,
    ) {
        let Some(inst) = self.popup_instances.remove(&instance_id) else {
            return;
        };
        // plugin process가 살아있을 때만 알린다. 종료 중이면 다음 spawn에서 새 인스턴스 id로 시작.
        if self.processes.contains_key(&inst.plugin_id) {
            self.send_surface_request(
                &inst.plugin_id,
                protocol::METHOD_POPUP_CLOSED,
                json!({
                    "instance_id": instance_id,
                    "reason": reason,
                }),
                PendingRequestKind::Other,
            );
        }
    }

    /// 현재 활성 popup 인스턴스 목록. PopupManager 렌더 / debug IPC가 사용.
    pub fn popup_instances(&self) -> impl Iterator<Item = (u64, &PopupInstance)> {
        self.popup_instances.iter().map(|(k, v)| (*k, v))
    }

    /// 단계 G: 사용자 단축키 매칭으로 plugin command를 trigger. 응답은
    /// `SurfaceResult` 형태로 받아 tree/display_name을 갱신할 수 있다.
    pub fn send_command_invoke(&mut self, plugin_id: &str, surface_id: u32, command_id: &str) {
        if !self.processes.contains_key(plugin_id) {
            tracing::warn!(
                "command.invoke: plugin '{}' is not running, dropping command '{}'",
                plugin_id,
                command_id
            );
            return;
        }
        self.send_surface_request(
            plugin_id,
            protocol::METHOD_COMMAND_INVOKE,
            json!({
                "surface_id": surface_id,
                "command_id": command_id,
            }),
            PendingRequestKind::CommandInvoke { surface_id },
        );
    }
}
