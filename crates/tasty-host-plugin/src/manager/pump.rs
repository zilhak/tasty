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
use tasty_plugin_protocol::SharedBufferId;

use super::{
    AUTO_RELOAD_POLL_INTERVAL, HEALTHCHECK_TIMEOUT, PING_INTERVAL, PendingPluginCall,
    PendingRequestKind, PluginManager, RemoteSurfaceEntry,
};

/// 한 tick 의 plugin→호스트 이벤트 수집 결과.
///
/// `pump` 이 `collect_plugin_events` 로 채운 뒤 `apply_collected_events` 로
/// 소비한다. 각 `Vec` 은 원본 pump 의 누산기와 1:1 대응하며, 채워지는 순서·
/// 조건·처리 순서를 그대로 보존한다.
#[derive(Default)]
struct CollectedPluginEvents {
    hello_log: Vec<(String, String)>,
    to_register: Vec<String>,
    new_calls: Vec<PendingPluginCall>,
    new_event_publishes: Vec<(String, tasty_plugin_protocol::EventEnvelope)>,
    new_event_subscribes: Vec<(String, u64, String)>,
    new_event_unsubscribes: Vec<(String, u64)>,
    // egui-mesh paint_frame 알림 (A1-S3): (surface_id, frame 메타).
    new_paint_frames: Vec<(u32, super::EguiMeshFrame)>,
    // egui-mesh popup paint_frame 알림 (A2): (instance_id, frame 메타).
    new_popup_paint_frames: Vec<(u64, super::EguiMeshFrame)>,
    // egui-mesh banner paint_frame 알림 (A3): (instance_id, frame 메타).
    new_banner_paint_frames: Vec<(u64, super::EguiMeshFrame)>,
    // plugin 이 폐기한 shared buffer (성장 재생성 등): (plugin_id, buffer_id).
    // host 매핑을 해제하지 않으면 구세대 버퍼가 plugin 수명 내내 남는다.
    released_buffers: Vec<(String, SharedBufferId)>,
    // 프로세스가 죽으면(reader 스레드 종료 → event_tx drop) event_rx 가 Disconnected
    // 가 된다. 60초 healthcheck 보다 먼저 감지해, 죽은 plugin 의 egui-mesh frame 을
    // 즉시 비워 stale mesh 가 계속 합성되는 것을 막는다 (research-a1 §9-7 crash 격리).
    disconnected: Vec<String>,
}

impl PluginManager {
    /// 매 tick 호출. plugin 이벤트 처리 + 헬스체크 + 비응답 재시작.
    ///
    /// 반환: 본 tick 에서 *처음 hello 받은 plugin* 의 `(plugin_id, version)`
    /// 리스트. 호출자 (App) 가 `finalize_plugin_hello` 로 surface_kind registry
    /// 등록 + CoreEvent (PluginLoaded / PluginSurfaceKindRegistered) 발화를
    /// 처리한다 (D.3.C.G.2.e). 비어있으면 finalize 안 호출.
    pub fn pump(&mut self) -> Vec<(String, String)> {
        // 1. plugin → 호스트 이벤트 수집 후 일괄 처리 (수집 순서·부수효과 보존).
        let collected = self.collect_plugin_events();
        let hello_pairs = self.apply_collected_events(collected);

        // 2. 새로 만들어진 RemoteSurface 등록 + plugin에 surface.create/restore 송신.
        self.drain_host_cmds();

        // 4. plugin → 호스트 응답 처리 (display_name/snapshot 동기화).
        self.drain_plugin_responses();

        // 4a. 타임아웃된 extension hook을 fail-open 처리.
        self.sweep_expired_hooks();

        // 2. 주기적 ping
        self.send_periodic_ping();

        // 3. 헬스체크 — 60초 무응답 시 재시작
        self.restart_unresponsive_plugins();

        // H.f — auto-reload polling.
        self.poll_auto_reload();

        hello_pairs
    }

    /// plugin→호스트 이벤트를 `processes` 순회로 수집. self 를 읽기만 하며
    /// (부수효과는 `apply_collected_events` 에서), 각 프로세스의 큐를 순서대로
    /// 비운다 — 수집 순서를 원본 그대로 보존한다.
    fn collect_plugin_events(&self) -> CollectedPluginEvents {
        let mut out = CollectedPluginEvents::default();
        for (id, proc) in &self.processes {
            loop {
                match proc.event_rx.try_recv() {
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        out.disconnected.push(id.clone());
                        break;
                    }
                    Ok(ev) => self.classify_event(id, ev, &mut out),
                }
            }
        }
        out
    }

    /// 단일 `PluginEvent` 를 종류별 누산기로 분류. 부수효과 없이 `out` 에만
    /// push 하며(Log 만 즉시 로깅 — 원본 동일), 누산기 mutation 을 원본 arm 과
    /// 1:1 로 유지한다.
    fn classify_event(&self, id: &str, ev: PluginEvent, out: &mut CollectedPluginEvents) {
        match ev {
            PluginEvent::Hello { plugin_id, version } => {
                out.hello_log.push((plugin_id.clone(), version));
                if !self.registered_plugins.contains(&plugin_id) {
                    out.to_register.push(plugin_id);
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
                out.new_paint_frames.push((
                    surface_id,
                    super::EguiMeshFrame {
                        plugin_id: id.to_string(),
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
                out.new_popup_paint_frames.push((
                    instance_id,
                    super::EguiMeshFrame {
                        plugin_id: id.to_string(),
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
                out.new_banner_paint_frames.push((
                    instance_id,
                    super::EguiMeshFrame {
                        plugin_id: id.to_string(),
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
                out.new_calls.push(PendingPluginCall {
                    plugin_id: id.to_string(),
                    call_id,
                    method,
                    params,
                    permissions: perms,
                });
            }
            PluginEvent::EventPublish { envelope } => {
                out.new_event_publishes.push((id.to_string(), envelope));
            }
            PluginEvent::EventSubscribe { sub_id, pattern } => {
                out.new_event_subscribes
                    .push((id.to_string(), sub_id, pattern));
            }
            PluginEvent::EventUnsubscribe { sub_id } => {
                out.new_event_unsubscribes.push((id.to_string(), sub_id));
            }
            PluginEvent::SharedBufferReleased { id: buffer_id } => {
                out.released_buffers.push((id.to_string(), buffer_id));
            }
            PluginEvent::Unknown => {
                // forward-compat fallback — 신버전 plugin 의 미지 이벤트는 무시.
                tracing::debug!("[plugin {}] unknown event kind (ignored)", id);
            }
        }
    }

    /// 수집된 이벤트를 원본 pump 와 동일한 순서로 처리 (부수효과 확정).
    /// 반환: 본 tick 에서 처음 hello 받은 plugin 의 `(plugin_id, version)`.
    fn apply_collected_events(
        &mut self,
        collected: CollectedPluginEvents,
    ) -> Vec<(String, String)> {
        let CollectedPluginEvents {
            hello_log,
            to_register,
            new_calls,
            new_event_publishes,
            new_event_subscribes,
            new_event_unsubscribes,
            new_paint_frames,
            new_popup_paint_frames,
            new_banner_paint_frames,
            released_buffers,
            disconnected,
        } = collected;

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
        for (plugin_id, buffer_id) in released_buffers {
            self.release_plugin_buffer(&plugin_id, buffer_id);
        }
        for (plugin_id, version) in hello_log {
            tracing::info!("plugin hello: {} v{}", plugin_id, version);
            // drift 감지: 바이너리(hello 보고 버전)와 설치 매니페스트 버전 불일치.
            // dev bundle 은 매니페스트(소스)와 바이너리(target exe)를 독립적으로
            // copy_if_newer 하므로, plugin 을 재빌드하지 않으면 "최신 매니페스트 +
            // stale exe" 조합이 조용히 설치된다 — e2e markdown.recent 회귀의 원인.
            // 동작은 막지 않고(런타임 호환 판정은 api_version 몫) 소리만 낸다.
            if let Some(pkg) = self.packages.iter().find(|p| p.manifest.id == plugin_id)
                && pkg.manifest.version != version
            {
                tracing::warn!(
                    "plugin '{plugin_id}' version drift: binary v{version} != manifest v{} — \
                     stale build? (dev: `cargo build --workspace` 후 재실행)",
                    pkg.manifest.version
                );
            }
        }
        let hello_pairs = self.register_new_hellos(&to_register);

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

        hello_pairs
    }

    /// hello 를 처음 받은 plugin 의 권한 set / event_bus 패턴 / settings_pages 동기화.
    /// surface_kind registry 등록 + `registered_plugins.insert` 는 호출자
    /// (App::finalize_plugin_hello) 가 처리 — CoreEvent 발화 위치 정렬.
    fn register_new_hellos(&mut self, to_register: &[String]) -> Vec<(String, String)> {
        let mut hello_pairs: Vec<(String, String)> = Vec::new();
        if !to_register.is_empty() {
            for plugin_id in to_register {
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
        hello_pairs
    }

    /// 주기적 ping — `PING_INTERVAL` 경과 시 전 프로세스에 ping 송신.
    fn send_periodic_ping(&mut self) {
        if self.last_ping.elapsed() >= PING_INTERVAL {
            for proc in self.processes.values() {
                let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
                proc.ping(id);
            }
            self.last_ping = Instant::now();
        }
    }

    /// 헬스체크 — `HEALTHCHECK_TIMEOUT` 무응답 plugin 을 재시작.
    fn restart_unresponsive_plugins(&mut self) {
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
    }

    /// auto-reload polling. flag off 면 `check_for_updates` 가 즉시 빈 Vec 을
    /// 반환해 cost 0. flag on 이고 마지막 tick 으로부터 `AUTO_RELOAD_POLL_INTERVAL`
    /// 경과 시 1회 polling.
    fn poll_auto_reload(&mut self) {
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
    }

    /// Surface 닫힘 시 plugin surface 정리 — 소유 plugin 에 `surface.destroy` 를
    /// 보내(plugin 측 per-surface 상태 해제: docs/mesh 컨텍스트/캐시) host 측
    /// `RemoteSurfaceEntry`(shm 핸들)와 stale mesh frame 메타를 제거한다.
    /// plugin surface 가 아니면(터미널 등) no-op — 호출측은 kind 를 구분할 필요 없다.
    ///
    /// 소유 plugin 해석은 두 갈래다:
    /// 1. `RemoteSurfaceEntry` 가 있는 surface — entry 의 plugin_id.
    /// 2. egui-mesh surface(markdown 등) — entry 를 만들지 않으므로
    ///    (`send_egui_mesh_surface_create` 참조) manifest `[[surface_kinds]]` 의
    ///    kind 선언으로 owner 를 해석한다.
    ///
    /// 이 통지가 없으면 plugin 프로세스가 surface 상태를 영원히 들고 있어
    /// open/close 반복 시 무한 성장한다 (soak S6 실측: markdown 사이클당 ~30MB).
    /// plugin 이 create 를 받은 적 없는 surface 에 destroy 가 가도 plugin 측
    /// `destroy_surface` 는 맵 remove 뿐이라 무해하다.
    pub fn destroy_remote_surface(&mut self, surface_id: u32, kind: Option<&str>) {
        // 이 surface 의 mesh frame 이 참조하던 shared buffer 매핑도 host 측에서
        // 해제한다 — plugin 은 해제를 알릴 프로토콜 메시지가 없어 여기서 안 지우면
        // plugin 수명 내내 누적된다 (`release_plugin_buffer` 문서 참조).
        let frame = self.egui_mesh_frames.remove(&surface_id);
        if let Some(f) = &frame {
            let (pid, bid) = (f.plugin_id.clone(), f.buffer_id);
            self.release_plugin_buffer(&pid, bid);
        }
        if let Some(entry) = self.surfaces.remove(&surface_id) {
            self.send_surface_request(
                &entry.plugin_id,
                protocol::METHOD_SURFACE_DESTROY,
                json!({ "surface_id": surface_id }),
                PendingRequestKind::Other,
            );
            // entry drop → SurfaceHandles(shm) 해제.
            return;
        }
        // egui-mesh surface: 수신했던 mesh frame 의 plugin_id 가 1순위 owner 소스다 —
        // cascade 시점엔 surface 가 이미 layout 에서 제거돼 kind 가 None 으로 올 수
        // 있기 때문 (`cascade_surface_closed` 의 surface_kind 폴백 주석 참조).
        // frame 을 한 번도 못 받은 surface(paint 전 즉시 close)만 kind 선언으로 폴백.
        let owner = frame
            .map(|f| f.plugin_id)
            .or_else(|| kind.and_then(|k| self.plugin_id_for_surface_kind(k)));
        if let Some(pid) = owner {
            tracing::debug!("surface.destroy → plugin '{pid}' (surface {surface_id})");
            self.send_surface_request(
                &pid,
                protocol::METHOD_SURFACE_DESTROY,
                json!({ "surface_id": surface_id }),
                PendingRequestKind::Other,
            );
        } else {
            tracing::debug!(
                "surface.destroy skipped (surface {surface_id}, kind {kind:?} — owner 미해석)"
            );
        }
    }

    /// manifest `[[surface_kinds]]` 가 `kind` 를 선언한 plugin id. egui-mesh
    /// surface 의 kind→owner 해석용. 없으면(터미널/호스트 빌트인) None.
    fn plugin_id_for_surface_kind(&self, kind: &str) -> Option<String> {
        self.packages
            .iter()
            .find(|p| p.manifest.surface_kinds.iter().any(|sk| sk.kind == kind))
            .map(|p| p.manifest.id.clone())
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
                    self.surfaces.insert(
                        surface_id,
                        RemoteSurfaceEntry {
                            plugin_id: plugin_id.clone(),
                            handles,
                        },
                    );
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
                    self.surfaces.insert(
                        surface_id,
                        RemoteSurfaceEntry {
                            plugin_id: plugin_id.clone(),
                            handles,
                        },
                    );
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
