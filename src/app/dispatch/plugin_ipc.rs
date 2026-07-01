//! plugin process 가 보낸 IPC 호출 dispatch.

use serde_json::json;

use crate::app::App;
use crate::ipc;

impl App {
    /// plugin process가 보낸 IPC 호출들을 라우터로 디스패치하고 결과를 plugin에 회신.
    /// CallerContext::Plugin으로 들어가므로 권한 게이트가 적용된다. 호출 메서드가
    /// 다른 plugin이 점유한 namespace prefix와 매칭되면 `forward_namespace_call_from_plugin`
    /// 경로로 우회 (응답은 target plugin이 줄 때까지 보류되며 main loop 다음 tick에서
    /// caller plugin에 `ipc.result`로 회신).
    pub(crate) fn process_plugin_ipc_calls(&mut self) {
        let calls = match self.plugin_manager.as_mut() {
            Some(mgr) => mgr.take_pending_plugin_calls(),
            None => return,
        };
        for call in calls {
            // shared buffer 생성은 main 채널 + 보조 채널을 동시에 다뤄야 해서
            // dispatcher에 노출하지 않고 매니저가 직접 처리한다. params에서 size를
            // 꺼내 manager에 위임 → 매니저가 fd/HANDLE 송신 + RPC 응답을 모두 처리.
            if call.method == tasty_plugin_protocol::METHOD_HOST_SHARED_BUFFER_CREATE {
                let size = call
                    .params
                    .get("size")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    let (result, error) =
                        match mgr.create_shared_buffer_for(&call.plugin_id, call.call_id, size) {
                            Ok(r) => (serde_json::to_value(&r).ok(), None),
                            Err(e) => (None, Some(e)),
                        };
                    mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
                }
                continue;
            }
            // popup.close 인터셉트 — PluginManager가 App에 있어 일반 라우터로 도달 불가.
            // ensure_allowed로 method_meta 권한 게이트(ui.popup)를 통과한 뒤,
            // instance_id가 호출자 plugin 소유인지 확인하고 PluginRequest 사유로 close.
            if call.method == "popup.close" {
                let caller = ipc::caller::CallerContext::Plugin {
                    plugin_id: call.plugin_id.clone(),
                    permissions: call.permissions.clone(),
                };
                let (result, error) = match caller.ensure_allowed(&call.method) {
                    Err(e) => (None, Some(e.to_string())),
                    Ok(()) => {
                        let instance_id = call.params.get("instance_id").and_then(|v| v.as_u64());
                        match instance_id {
                            None => (None, Some("popup.close: missing 'instance_id'".to_string())),
                            Some(id) => {
                                let mgr = self.plugin_manager.as_mut();
                                let owns = mgr
                                    .as_ref()
                                    .and_then(|m| {
                                        m.popup_instances()
                                            .find(|(iid, _)| *iid == id)
                                            .map(|(_, inst)| inst.plugin_id == call.plugin_id)
                                    })
                                    .unwrap_or(false);
                                if !owns {
                                    (
                                        None,
                                        Some(format!(
                                            "popup.close: instance {id} not owned by plugin '{}'",
                                            call.plugin_id
                                        )),
                                    )
                                } else if let Some(m) = mgr {
                                    m.close_popup_instance(
                                        id,
                                        tasty_plugin_protocol::PopupCloseReason::PluginRequest,
                                    );
                                    (Some(serde_json::Value::Object(Default::default())), None)
                                } else {
                                    (None, Some("popup.close: plugin manager unavailable".into()))
                                }
                            }
                        }
                    }
                };
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
                }
                continue;
            }
            // banner.open (A3) — plugin 이 자기 surface 에 egui-mesh 배너를 띄운다.
            // ui.banner 권한 게이트 + D1 소유권 검증(자기 surface 만)은 open_plugin_banner 가.
            if call.method == "banner.open" {
                let caller = ipc::caller::CallerContext::Plugin {
                    plugin_id: call.plugin_id.clone(),
                    permissions: call.permissions.clone(),
                };
                let (result, error) = match caller.ensure_allowed(&call.method) {
                    Err(e) => (None, Some(e.to_string())),
                    Ok(()) => {
                        let banner_id = call.params.get("banner_id").and_then(|v| v.as_str());
                        let surface_id = call.params.get("surface_id").and_then(|v| v.as_u64());
                        match (banner_id, surface_id) {
                            (Some(bid), Some(sid)) => {
                                let bid = bid.to_string();
                                match self.open_plugin_banner(
                                    Some(&call.plugin_id),
                                    &bid,
                                    sid as u32,
                                ) {
                                    Ok(iid) => (Some(json!({ "instance_id": iid })), None),
                                    Err(e) => (None, Some(e)),
                                }
                            }
                            _ => (
                                None,
                                Some(
                                    "banner.open: missing 'banner_id' or 'surface_id'".to_string(),
                                ),
                            ),
                        }
                    }
                };
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
                }
                continue;
            }
            // banner.close (A3) — plugin 이 자기 배너 인스턴스를 닫는다.
            if call.method == "banner.close" {
                let caller = ipc::caller::CallerContext::Plugin {
                    plugin_id: call.plugin_id.clone(),
                    permissions: call.permissions.clone(),
                };
                let (result, error) = match caller.ensure_allowed(&call.method) {
                    Err(e) => (None, Some(e.to_string())),
                    Ok(()) => match call.params.get("instance_id").and_then(|v| v.as_u64()) {
                        None => (
                            None,
                            Some("banner.close: missing 'instance_id'".to_string()),
                        ),
                        Some(iid) => {
                            if !self.plugin_owns_banner(&call.plugin_id, iid) {
                                (
                                    None,
                                    Some(format!(
                                        "banner.close: instance {iid} not owned by plugin '{}'",
                                        call.plugin_id
                                    )),
                                )
                            } else {
                                self.close_plugin_banner(
                                    iid,
                                    tasty_plugin_protocol::BannerCloseReason::PluginRequest,
                                );
                                (Some(json!({ "closed": iid })), None)
                            }
                        }
                    },
                };
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
                }
                continue;
            }
            // namespace forward 경로: 메서드가 다른 plugin의 prefix에 매칭되면
            // 검증/forward를 plugin_manager에 위임한다. 응답은 비동기.
            //
            // self-call(caller가 prefix owner와 동일)인 경우는 forward하지 않고
            // 호스트 dispatcher로 통과시킨다. plugin이 자기 namespace 메서드의
            // 구현을 호스트 본문에 위임하는 trampoline 패턴(예: com.tasty.image)을
            // 지원하기 위함. 호스트에 동명 메서드가 없으면 일반 -32601이 떨어진다.
            if let Some(mgr) = self.plugin_manager.as_mut()
                && let Some(owner) = mgr.ipc_namespaces.resolve(&call.method)
                && owner != call.plugin_id
            {
                mgr.forward_namespace_call_from_plugin(
                    &call.method,
                    call.params.clone(),
                    &call.plugin_id,
                    call.call_id,
                );
                continue;
            }
            let caller = ipc::caller::CallerContext::Plugin {
                plugin_id: call.plugin_id.clone(),
                permissions: call.permissions.clone(),
            };
            let request = ipc::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::Value::from(call.call_id)),
                method: call.method.clone(),
                params: call.params.clone(),
                session_token: None,
            };
            let response = self.dispatch_with_caller(&request, &caller);
            let (result, error) = match response.error {
                Some(err) => (None, Some(err.message)),
                None => (response.result, None),
            };
            if let Some(mgr) = self.plugin_manager.as_mut() {
                mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
            }
        }
    }
}
