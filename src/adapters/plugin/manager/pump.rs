//! 메인 루프 매 tick 에서 호출되는 `PluginManager::pump` + `drain_host_cmds`.
//!
//! - `pump`: plugin 알림 처리, healthcheck/PING, 호스트→plugin 핸드셰이크, surface 등록, restart.
//! - `drain_host_cmds`: registry/file_format/popup closure 가 큐잉한 `HostCmd` 일괄 처리.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::plugin::host_cmd::HostCmd;
use crate::plugin::manifest::{Permission, PluginPackage};
use crate::plugin::process::PluginProcess;
use crate::plugin::protocol::{self, PluginEvent};

use super::{
    HEALTHCHECK_TIMEOUT, PING_INTERVAL, PendingPluginCall, PendingRequestKind, PluginManager,
    RemoteSurfaceEntry,
};

impl PluginManager {
    /// 매 tick 호출. plugin 이벤트 처리 + 헬스체크 + 비응답 재시작.
    ///
    /// 반환: 본 tick 에서 *처음 hello 받은 plugin* 의 `(plugin_id, version)`
    /// 리스트. 호출자 (App) 가 `finalize_plugin_hello` 로 surface_kind registry
    /// 등록 + CoreEvent (PluginLoaded / PluginSurfaceKindRegistered) 발화를
    /// 처리한다 (D.3.C.G.2.e). 비어있으면 finalize 안 호출.
    pub fn pump(&mut self) -> Vec<(String, String)> {
        // 1. plugin → 호스트 이벤트 처리
        let mut hello_log: Vec<(String, String)> = Vec::new();
        let mut to_register: Vec<String> = Vec::new();
        let mut new_calls: Vec<PendingPluginCall> = Vec::new();
        let mut new_event_publishes: Vec<(String, tasty_plugin_protocol::EventEnvelope)> =
            Vec::new();
        let mut new_event_subscribes: Vec<(String, u64, String)> = Vec::new();
        let mut new_event_unsubscribes: Vec<(String, u64)> = Vec::new();
        for (id, proc) in &self.processes {
            while let Ok(ev) = proc.event_rx.try_recv() {
                match ev {
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
                }
            }
        }
        if !new_calls.is_empty() {
            self.pending_plugin_calls.extend(new_calls);
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

        // 3. RemoteSurface가 모은 사용자 이벤트 → plugin에 surface.event 송신.
        self.flush_pending_events();

        // 4. plugin → 호스트 응답 처리 (tree 동기화).
        self.drain_plugin_responses();

        // 4a. 타임아웃된 extension hook을 fail-open 처리.
        self.sweep_expired_hooks();

        // 4b. Event Bus throttle: 만료된 pending envelope 발화.
        self.pump_throttled_events();

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
            self.event_bus.clear_plugin(&id);
            self.cancel_pending_namespace_calls(&id, "plugin restarting");
            self.plugin_buffers.remove(&id);
            if let Some(pkg) = self.packages.iter().find(|p| p.manifest.id == id).cloned() {
                self.start_plugin_internal(&pkg);
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
                    self.send_surface_request(
                        &plugin_id,
                        protocol::METHOD_SURFACE_CREATE,
                        json!({
                            "surface_id": surface_id,
                            "kind": kind,
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
