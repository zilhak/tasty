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
    /// `popup.open` IPC를 보낸다.
    ///
    /// 반환값은 호스트가 발급한 instance_id. 추후 `close_popup_instance`에
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
        if let Some(existing) = self.find_open_popup_instance(plugin_id, popup_id) {
            return Some(existing);
        }
        let contribute = self.resolve_open_popup_contribute(plugin_id, popup_id)?;
        let instance_id = self.next_popup_instance_id;
        self.next_popup_instance_id = self.next_popup_instance_id.wrapping_add(1);
        self.popup_instances.insert(
            instance_id,
            PopupInstance {
                plugin_id: plugin_id.to_string(),
                popup_id: popup_id.to_string(),
                contribute,
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

    /// 단일 인스턴스 가드: 같은 (plugin_id, popup_id) 인스턴스가 이미 열려 있으면 그 id.
    /// 사용자 조작(popup open)이 중복 인스턴스를 쌓지 않게 하는 host 소유 정책
    /// (identity 원칙 1 — 셸 생명주기는 host 소유).
    fn find_open_popup_instance(&self, plugin_id: &str, popup_id: &str) -> Option<u64> {
        let (existing, _) = self
            .popup_instances
            .iter()
            .find(|(_, inst)| inst.plugin_id == plugin_id && inst.popup_id == popup_id)?;
        tracing::debug!(
            "popup.open: '{plugin_id}/{popup_id}' already open (instance {existing}); dedup"
        );
        Some(*existing)
    }

    /// 매니페스트에서 contribute 를 찾고 egui-mesh api_version 게이트까지 통과해야 `Some`.
    /// egui-mesh popup 은 epaint 와이어가 host·plugin 동일 컴파일을 강제하므로
    /// api_version 일치를 게이트한다(surface egui-mesh 등록 정책 미러, ADR-0028).
    fn resolve_open_popup_contribute(
        &self,
        plugin_id: &str,
        popup_id: &str,
    ) -> Option<tasty_plugin_manifest::PopupContribute> {
        let pkg = self.packages.iter().find(|p| p.manifest.id == plugin_id)?;
        let contribute = pkg
            .manifest
            .contributes
            .popup
            .iter()
            .find(|p| p.id == popup_id)
            .cloned()?;
        if contribute.rendering == tasty_plugin_manifest::PopupRendering::EguiMesh
            && pkg.manifest.api_version != tasty_plugin_manifest::HOST_API_VERSION
        {
            tracing::warn!(
                "popup.open: egui-mesh popup '{plugin_id}/{popup_id}' has api_version '{}' \
                 incompatible with host '{}'; ignoring",
                pkg.manifest.api_version,
                tasty_plugin_manifest::HOST_API_VERSION
            );
            return None;
        }
        Some(contribute)
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
        // egui-mesh popup 이면 합성기가 참조하던 frame 메타를 함께 정리 (stale buffer 방지).
        // 그 frame 의 shared buffer 매핑도 해제 — 안 지우면 plugin 수명 내내 host 에
        // 누적된다 (`release_plugin_buffer` 문서 참조).
        if let Some(f) = self.popup_mesh_frames.remove(&instance_id) {
            self.release_plugin_buffer(&f.plugin_id, f.buffer_id);
        }
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
