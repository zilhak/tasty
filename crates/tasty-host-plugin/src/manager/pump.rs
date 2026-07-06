//! 메인 루프 매 tick 에서 호출되는 `PluginManager::pump` + `drain_host_cmds`.
//!
//! - `pump`: plugin 알림 처리, healthcheck/PING, 호스트→plugin 핸드셰이크, surface 등록, restart.
//! - `drain_host_cmds`: registry/file_format/popup closure 가 큐잉한 `HostCmd` 일괄 처리.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::host_cmd::HostCmd;
use crate::protocol::{self, PluginEvent};
use tasty_plugin_manifest::Permission;

use super::{
    AUTO_RELOAD_POLL_INTERVAL, HEALTHCHECK_TIMEOUT, PING_INTERVAL, PendingPluginCall,
    PendingRequestKind, PluginManager, RemoteSurfaceEntry,
};

impl PluginManager {
    /// 매 tick 호출. plugin 이벤트 처리 + 헬스체크 + 비응답 재시작.
    ///
    /// 반환: 본 tick 에서 *처음 hello 받은 plugin* 의 `(plugin_id, version)`
    /// 리스트. 호출자 (App) 가 `finalize_plugin_hello` 로 surface_kind registry
    /// 등록 + CoreEvent (PluginLoaded / PluginSurfaceKindRegistered) 발화를
    /// 처리한다 (D.3.C.G.2.e). 비어있으면 finalize 안 호출.
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: 리팩터 후보 — plugin→host 이벤트 펌프(hello/call/publish/register 다단계 수집). 게이트 도입과 별건
    pub fn pump(&mut self) -> Vec<(String, String)> {
        // 1. plugin → 호스트 이벤트 처리
        let mut hello_log: Vec<(String, String)> = Vec::new();
        let mut to_register: Vec<String> = Vec::new();
        let mut new_calls: Vec<PendingPluginCall> = Vec::new();
        let mut new_event_publishes: Vec<(String, tasty_plugin_protocol::EventEnvelope)> =
            Vec::new();
        let mut new_event_subscribes: Vec<(String, u64, String)> = Vec::new();
        let mut new_event_unsubscribes: Vec<(String, u64)> = Vec::new();
        // egui-mesh paint_frame 알림 (A1-S3): (surface_id, frame 메타).
        let mut new_paint_frames: Vec<(u32, super::EguiMeshFrame)> = Vec::new();
        // egui-mesh popup paint_frame 알림 (A2): (instance_id, frame 메타).
        let mut new_popup_paint_frames: Vec<(u64, super::EguiMeshFrame)> = Vec::new();
        // egui-mesh banner paint_frame 알림 (A3): (instance_id, frame 메타).
        let mut new_banner_paint_frames: Vec<(u64, super::EguiMeshFrame)> = Vec::new();
        // 프로세스가 죽으면(reader 스레드 종료 → event_tx drop) event_rx 가 Disconnected
        // 가 된다. 60초 healthcheck 보다 먼저 감지해, 죽은 plugin 의 egui-mesh frame 을
        // 즉시 비워 stale mesh 가 계속 합성되는 것을 막는다 (research-a1 §9-7 crash 격리).
        let mut disconnected: Vec<String> = Vec::new();
        for (id, proc) in &self.processes {
            loop {
                match proc.event_rx.try_recv() {
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected.push(id.clone());
                        break;
                    }
                    Ok(ev) => match ev {
                        PluginEvent::Hello { plugin_id, version } => {
                            hello_log.push((plugin_id.clone(), version));
                            if !self.registered_plugins.contains(&plugin_id) {
                                to_register.push(plugin_id);
                            }
                        }
                        PluginEvent::Log { level, message } => match level.as_str() {
                            "error" => tracing::error!("[plugin {}] {}", id, message),
                            "warn" => tracing::warn!("[plugin {}] {}", id, message),
                            _ => tracing::info!("[plugin {}] {}", id, message),
                        },
                        PluginEvent::SurfaceInvalidated { .. } => {
                            // 단계 06에서 처리
                        }
                        PluginEvent::PaintFrame {
                            surface_id,
                            buffer_id,
                            generation,
                            frame_seq,
                            full_textures,
                        } => {
                            // A1-S3 수신 라우팅: 최근 mesh frame 메타를 저장. 렌더 prepare(A1-S5)가
                            // buffer lookup + 디코드 출발점으로 읽는다. redraw 는 수신 스레드가
                            // 매 라인마다 waker 를 깨우므로 별도 트리거 불필요.
                            new_paint_frames.push((
                                surface_id,
                                super::EguiMeshFrame {
                                    plugin_id: id.clone(),
                                    buffer_id,
                                    generation,
                                    frame_seq,
                                    full_textures,
                                },
                            ));
                        }
                        PluginEvent::PopupPaintFrame {
                            instance_id,
                            buffer_id,
                            generation,
                            frame_seq,
                            full_textures,
                        } => {
                            // A2 popup 수신 라우팅: 최근 popup mesh frame 메타를 저장.
                            // host 합성기(popup_mesh_render)가 instance_id 로 lookup 한다.
                            new_popup_paint_frames.push((
                                instance_id,
                                super::EguiMeshFrame {
                                    plugin_id: id.clone(),
                                    buffer_id,
                                    generation,
                                    frame_seq,
                                    full_textures,
                                },
                            ));
                        }
                        PluginEvent::BannerPaintFrame {
                            instance_id,
                            buffer_id,
                            generation,
                            frame_seq,
                            full_textures,
                        } => {
                            // A3 banner 수신 라우팅: 최근 banner mesh frame 메타를 저장.
                            // host 합성기(render_egui_mesh_banners)가 instance_id 로 lookup 한다.
                            new_banner_paint_frames.push((
                                instance_id,
                                super::EguiMeshFrame {
                                    plugin_id: id.clone(),
                                    buffer_id,
                                    generation,
                                    frame_seq,
                                    full_textures,
                                },
                            ));
                        }
                        PluginEvent::NotifyHost { .. } => {
                            // 단계 06에서 처리
                        }
                        PluginEvent::IpcCall {
                            call_id,
                            method,
                            params,
                        } => {
                            let perms = self
                                .plugin_permissions
                                .get(id)
                                .cloned()
                                .unwrap_or_else(|| Arc::new(HashSet::new()));
                            new_calls.push(PendingPluginCall {
                                plugin_id: id.clone(),
                                call_id,
                                method,
                                params,
                                permissions: perms,
                            });
                        }
                        PluginEvent::EventPublish { envelope } => {
                            new_event_publishes.push((id.clone(), envelope));
                        }
                        PluginEvent::EventSubscribe { sub_id, pattern } => {
                            new_event_subscribes.push((id.clone(), sub_id, pattern));
                        }
                        PluginEvent::EventUnsubscribe { sub_id } => {
                            new_event_unsubscribes.push((id.clone(), sub_id));
                        }
                    },
                }
            }
        }
        // 죽은 plugin 의 egui-mesh frame 을 즉시 비운다 — 60초 healthcheck 를 기다리지
        // 않고 surface 를 blank 로 전환해 stale mesh 합성을 막는다 (research-a1 §9-7).
        for dead in &disconnected {
            self.egui_mesh_frames.retain(|_, f| &f.plugin_id != dead);
            self.popup_mesh_frames.retain(|_, f| &f.plugin_id != dead);
            self.banner_mesh_frames.retain(|_, f| &f.plugin_id != dead);
        }
        if !new_calls.is_empty() {
            self.pending_plugin_calls.extend(new_calls);
        }
        for (surface_id, frame) in new_paint_frames {
            self.egui_mesh_frames.insert(surface_id, frame);
        }
        for (instance_id, frame) in new_popup_paint_frames {
            self.popup_mesh_frames.insert(instance_id, frame);
        }
        for (instance_id, frame) in new_banner_paint_frames {
            self.banner_mesh_frames.insert(instance_id, frame);
        }
        for (plugin_id, version) in hello_log {
            tracing::info!("plugin hello: {} v{}", plugin_id, version);
        }
        // hello 를 처음 받은 plugin 의 권한 set / event_bus 패턴 동기화.
        // surface_kind registry 등록 + `registered_plugins.insert` 는 호출자
        // (App::finalize_plugin_hello) 가 처리 — CoreEvent 발화 위치 정렬.
        let mut hello_pairs: Vec<(String, String)> = Vec::new();
        if !to_register.is_empty() {
            for plugin_id in &to_register {
                if let Some(pkg) = self.packages.iter().find(|p| &p.manifest.id == plugin_id) {
                    let granted = self.config.granted_permissions(plugin_id);
                    let perms: HashSet<Permission> = pkg
                        .manifest
                        .parsed_permissions()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|p| granted.contains(&p.as_token()))
                        .collect();
                    self.plugin_permissions
                        .insert(plugin_id.clone(), Arc::new(perms));
                    self.event_bus.set_plugin_permissions(
                        plugin_id,
                        pkg.manifest.event_subscribe.clone(),
                        pkg.manifest.event_publish.clone(),
                    );
                    // settings_pages: hello/manifest 수신 시 plugin 의 sub-page 등록.
                    // 동일 plugin 의 중복 register 방지를 위해 먼저 정리한 뒤 register.
                    self.settings_pages.unregister_plugin(plugin_id);
                    self.settings_pages.register(
                        plugin_id.clone(),
                        pkg.manifest.contributes.settings_pages.clone(),
                    );
                    hello_pairs.push((plugin_id.clone(), pkg.manifest.version.clone()));
                }
            }
        }

        // Event Bus: plugin이 보낸 subscribe/unsubscribe/publish 처리.
        for (plugin_id, sub_id, pattern) in new_event_subscribes {
            if let Err(e) = self
                .event_bus
                .subscribe_plugin(&plugin_id, sub_id, pattern.clone())
            {
                tracing::warn!("plugin '{plugin_id}' event.subscribe rejected: {e}");
            }
        }
        for (plugin_id, sub_id) in new_event_unsubscribes {
            self.event_bus.unsubscribe_plugin(&plugin_id, sub_id);
        }
        for (plugin_id, envelope) in new_event_publishes {
            self.route_plugin_event_publish(&plugin_id, envelope);
        }

        // 2. 새로 만들어진 RemoteSurface 등록 + plugin에 surface.create/restore 송신.
        self.drain_host_cmds();

        // 4. plugin → 호스트 응답 처리 (display_name/snapshot 동기화).
        self.drain_plugin_responses();

        // 4a. 타임아웃된 extension hook을 fail-open 처리.
        self.sweep_expired_hooks();

        // 2. 주기적 ping
        if self.last_ping.elapsed() >= PING_INTERVAL {
            for proc in self.processes.values() {
                let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
                proc.ping(id);
            }
            self.last_ping = Instant::now();
        }

        // 3. 헬스체크 — 60초 무응답 시 재시작
        let unresponsive: Vec<String> = self
            .processes
            .iter()
            .filter_map(|(id, p)| {
                if p.since_last_pong() > HEALTHCHECK_TIMEOUT {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        for id in unresponsive {
            tracing::warn!(
                "plugin '{}' unresponsive for {}s — restarting",
                id,
                HEALTHCHECK_TIMEOUT.as_secs()
            );
            {
                use tasty_plugin_protocol::EventScope;
                use tasty_plugin_protocol::events::payloads::PluginError;
                let payload = PluginError {
                    plugin_id: id.clone(),
                    error_kind: "unresponsive".to_string(),
                    message: format!(
                        "plugin '{}' did not respond to ping for {}s — restarting",
                        id,
                        HEALTHCHECK_TIMEOUT.as_secs()
                    ),
                };
                self.emit_host_event("plugin.error", &payload, EventScope::System);
            }
            if let Some(proc) = self.processes.remove(&id) {
                proc.shutdown(Duration::from_secs(2));
            }
            self.ipc_namespaces.unregister_plugin(&id);
            // G.D.b — runtime registry 도 mirror 해제. restart 후
            // start_plugin_internal 이 다시 register 한다.
            if let Some(pkg) = self.packages.iter().find(|p| p.manifest.id == id) {
                for ns in &pkg.manifest.contributes.ipc_namespace {
                    tasty_ipc::method_meta::unregister_plugin_prefix(&ns.prefix);
                }
            }
            self.event_bus.clear_plugin(&id);
            self.cancel_pending_namespace_calls(&id, "plugin restarting");
            self.plugin_buffers.remove(&id);
            // egui-mesh: 죽은 plugin 의 buffer 를 가리키는 stale frame 메타 제거 (A1-S3 / A2 / A3).
            self.egui_mesh_frames.retain(|_, f| f.plugin_id != id);
            self.popup_mesh_frames.retain(|_, f| f.plugin_id != id);
            self.banner_mesh_frames.retain(|_, f| f.plugin_id != id);
            // 죽은 plugin 의 banner 인스턴스도 정리 — 다음 spawn 에서 새 인스턴스로 시작.
            self.banner_instances.retain(|_, inst| inst.plugin_id != id);
            self.settings_pages.unregister_plugin(&id);
            if let Some(pkg) = self.packages.iter().find(|p| p.manifest.id == id).cloned() {
                self.start_plugin_internal(&pkg);
            }
        }

        // H.f — auto-reload polling. flag off 면 check_for_updates 가 즉시
        // 빈 Vec 을 반환해 cost 0. flag on 이고 마지막 tick 으로부터
        // AUTO_RELOAD_POLL_INTERVAL 경과 시 1회 polling.
        if self.auto_reload_enabled
            && self.last_auto_reload_check.elapsed() >= AUTO_RELOAD_POLL_INTERVAL
        {
            self.last_auto_reload_check = Instant::now();
            for plugin_id in self.check_for_updates() {
                if let Err(e) = self.auto_reload_one(&plugin_id) {
                    tracing::warn!("auto-reload '{plugin_id}' failed: {e}");
                }
            }
        }

        hello_pairs
    }

    pub(super) fn drain_host_cmds(&mut self) {
        loop {
            let cmd = match self.host_cmd_rx.try_recv() {
                Ok(c) => c,
                Err(_) => break,
            };
            match cmd {
                HostCmd::RemoteSurfaceCreated {
                    surface_id,
                    plugin_id,
                    kind,
                    cwd,
                    params,
                    handles,
                } => {
                    self.surfaces
                        .insert(surface_id, RemoteSurfaceEntry { handles });
                    let cwd_str = cwd.as_ref().and_then(|p| p.to_str()).map(str::to_string);
                    self.send_surface_request(
                        &plugin_id,
                        protocol::METHOD_SURFACE_CREATE,
                        json!({
                            "surface_id": surface_id,
                            "kind": kind,
                            "cwd": cwd_str,
                            "params": params,
                        }),
                        PendingRequestKind::SurfaceCreate { surface_id },
                    );
                }
                HostCmd::RemoteSurfaceRestored {
                    surface_id,
                    plugin_id,
                    kind,
                    data,
                    handles,
                } => {
                    self.surfaces
                        .insert(surface_id, RemoteSurfaceEntry { handles });
                    self.send_surface_request(
                        &plugin_id,
                        protocol::METHOD_SURFACE_RESTORE,
                        json!({
                            "surface_id": surface_id,
                            "kind": kind,
                            "data": data,
                        }),
                        PendingRequestKind::SurfaceRestore { surface_id },
                    );
                }
            }
        }
    }
}
