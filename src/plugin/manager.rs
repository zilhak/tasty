//! Plugin 생명주기 매니저.
//!
//! 호스트의 부팅 시 한 번 만들어지고, `App`이 유일한 인스턴스를 보유한다.
//! - 부팅 시 `discover_and_start()`로 `~/.tasty/plugins/`를 스캔하여 활성 plugin 모두 spawn
//! - 매 메인 루프 tick에서 `pump()` 호출 → plugin 알림 처리 + 헬스체크 + 재시작
//! - 종료 시 `shutdown_all()`

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::plugin::host_cmd::{HostCmd, SurfaceHandles};
use crate::plugin::ipc_namespace::IpcNamespaceRegistry;
use crate::plugin::listener::HostListener;
use crate::plugin::manifest::{ObserverEvent, Permission, PluginPackage};
use crate::plugin::process::PluginProcess;
use crate::plugin::protocol::{
    self, IpcCallResult, PluginEvent, PluginRequest, PluginResponse, SurfaceCloseReason,
    SurfaceLifecycleEvent, SurfaceResult,
};
use crate::plugin::registry_state::PluginsConfig;
use crate::surface_registry::SurfaceKindRegistry;

const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(60);
const PING_INTERVAL: Duration = Duration::from_secs(15);
const RESTART_FAILURE_WINDOW: Duration = Duration::from_secs(10);
const RESTART_FAILURE_LIMIT: usize = 3;

/// pending host→plugin request의 종류. 응답 수신 시 어떤 후처리를 할지 식별.
#[allow(dead_code)]
enum PendingRequestKind {
    SurfaceCreate { surface_id: u32 },
    SurfaceEvent { surface_id: u32 },
    SurfaceRestore { surface_id: u32 },
    /// 단계 G: 단축키 매칭으로 plugin command가 트리거된 경우. 응답은
    /// SurfaceResult 형태로 surface tree/display_name을 갱신할 수 있다.
    CommandInvoke { surface_id: u32 },
    Ping,
    /// 그 외 (host.hello 등) — 응답 무시.
    Other,
    /// Client IPC 요청을 plugin namespace로 forward한 경우. plugin이 응답을 주면
    /// 보관한 response_tx로 client에 회신한다.
    NamespaceInvoke {
        plugin_id: String,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        original_id: serde_json::Value,
    },
    /// 다른 plugin이 보낸 IpcCall이 namespace 메서드인 경우. target plugin이 응답을
    /// 주면 caller plugin에 `ipc.result`로 회신한다.
    PluginToPluginNamespace {
        /// forward 받은 target plugin (응답을 주는 쪽).
        plugin_id: String,
        /// 호출한 plugin (응답을 받을 쪽).
        caller_plugin_id: String,
        /// caller plugin이 ipc.call 시점에 발급한 call_id.
        call_id: u64,
    },
    /// surface.lifecycle broadcast. 응답은 fire-and-forget이라 폐기. surface_id는
    /// 디버그 로그용.
    SurfaceLifecycle { surface_id: u32 },
}

struct RemoteSurfaceEntry {
    plugin_id: String,
    #[allow(dead_code)]
    kind: String,
    handles: SurfaceHandles,
}

pub struct PluginManager {
    pub packages: Vec<PluginPackage>,
    pub processes: HashMap<String, PluginProcess>,
    pub config: PluginsConfig,
    waker: tasty_core::SharedWakerFactory,
    listener: Option<HostListener>,
    pub log_dir: PathBuf,
    next_request_id: AtomicU64,
    last_ping: Instant,
    /// plugin id → 최근 spawn 실패 timestamps. 짧은 시간 내 반복 실패하면 자동 disable.
    spawn_failures: HashMap<String, Vec<Instant>>,
    /// 자동 disable되어 사용자가 수동 enable하기 전까지 더 이상 spawn 시도 안 함.
    auto_disabled: std::collections::HashSet<String>,
    /// hello 받은 plugin의 surface_kinds를 등록하기 위한 registry 핸들. None이면
    /// registry 등록 동작이 비활성 (헤드리스/테스트).
    pub surface_registry: Option<Arc<SurfaceKindRegistry>>,
    /// 이미 registry에 등록된 plugin id (hello를 여러 번 받아도 1회만 등록).
    registered_plugins: std::collections::HashSet<String>,
    /// registry create/restore closure가 새 RemoteSurface 등록을 보내는 채널.
    host_cmd_tx: Sender<HostCmd>,
    host_cmd_rx: Receiver<HostCmd>,
    /// surface_id → RemoteSurface handle. 라이프사이클 동안 유지.
    surfaces: HashMap<u32, RemoteSurfaceEntry>,
    /// host → plugin 요청 ID → 종류. 응답 수신 시 후처리 dispatch용.
    pending_requests: HashMap<u64, PendingRequestKind>,
    /// 각 plugin에 grant된 권한. 매니페스트 + plugins.toml의 granted를 교집합한 결과.
    /// `Arc`로 공유하여 CallerContext가 동시 호출 시 안전.
    plugin_permissions: HashMap<String, Arc<HashSet<Permission>>>,
    /// plugin이 보낸 IpcCall을 호스트의 main loop에서 라우팅 처리하기 위해 모으는 큐.
    /// `App::process_plugin_ipc_calls()`가 매 tick에 비운다.
    pending_plugin_calls: Vec<PendingPluginCall>,
    /// plugin이 매니페스트로 선언한 단축키 command 일람. plugin
    /// enable/disable/install/remove 시 갱신됨.
    pub command_registry: super::command_registry::PluginCommandRegistry,
    /// plugin이 매니페스트로 선언한 IPC namespace prefix 일람. plugin이
    /// 실행 중일 때만 등록되며, 호스트 IPC dispatcher가 namespace 메서드를
    /// 어느 plugin에 forward할지 해결할 때 조회한다.
    pub ipc_namespaces: IpcNamespaceRegistry,
    /// surface lifecycle 구독자 일람. event → 구독 plugin id 집합. plugin이
    /// 실행 중일 때만 등록된다. `notify_surface_closed`가 이 set을 순회해 broadcast.
    surface_observers: HashMap<ObserverEvent, HashSet<String>>,
}

/// plugin → host IPC 호출 한 건. 라우팅 후 결과를 plugin에 회신해야 함.
#[derive(Debug, Clone)]
pub struct PendingPluginCall {
    pub plugin_id: String,
    pub call_id: u64,
    pub method: String,
    pub params: serde_json::Value,
    pub permissions: Arc<HashSet<Permission>>,
}

impl PluginManager {
    pub fn new(waker: tasty_core::SharedWakerFactory) -> Self {
        let log_dir = tasty_core::paths::tasty_home()
            .map(|d| d.join("plugins-logs"))
            .unwrap_or_else(|| PathBuf::from("./plugin-logs"));
        let _ = std::fs::create_dir_all(&log_dir);
        let (host_cmd_tx, host_cmd_rx) = mpsc::channel();
        Self {
            packages: Vec::new(),
            processes: HashMap::new(),
            config: PluginsConfig::load(),
            waker,
            listener: None,
            log_dir,
            next_request_id: AtomicU64::new(1),
            last_ping: Instant::now(),
            spawn_failures: HashMap::new(),
            auto_disabled: std::collections::HashSet::new(),
            surface_registry: None,
            registered_plugins: std::collections::HashSet::new(),
            host_cmd_tx,
            host_cmd_rx,
            surfaces: HashMap::new(),
            pending_requests: HashMap::new(),
            plugin_permissions: HashMap::new(),
            pending_plugin_calls: Vec::new(),
            command_registry: super::command_registry::PluginCommandRegistry::new(),
            ipc_namespaces: IpcNamespaceRegistry::new(),
            surface_observers: HashMap::new(),
        }
    }

    /// plugin에 grant된 권한 set을 갱신. 매니페스트 hello 시점 또는 사용자가
    /// grant/revoke 했을 때 호출. plugin process 재시작 없이 즉시 반영된다.
    pub fn set_plugin_permissions(&mut self, plugin_id: &str, perms: HashSet<Permission>) {
        self.plugin_permissions
            .insert(plugin_id.to_string(), Arc::new(perms));
    }

    /// plugin의 현재 권한 set. 등록되지 않은 plugin은 빈 set.
    /// (외부 caller surface로 노출 — `process_plugin_ipc_calls`는 이미 PendingPluginCall에
    /// 권한을 cache해 사용하므로 직접 호출자는 없다.)
    #[allow(dead_code)]
    pub fn plugin_permissions(&self, plugin_id: &str) -> Arc<HashSet<Permission>> {
        self.plugin_permissions
            .get(plugin_id)
            .cloned()
            .unwrap_or_else(|| Arc::new(HashSet::new()))
    }

    /// 호스트 main loop이 라우팅하기 위해 plugin IPC 호출을 모두 가져간다.
    pub fn take_pending_plugin_calls(&mut self) -> Vec<PendingPluginCall> {
        std::mem::take(&mut self.pending_plugin_calls)
    }

    /// 라우터가 처리한 결과를 plugin에 송신.
    pub fn send_ipc_result(
        &mut self,
        plugin_id: &str,
        call_id: u64,
        result: Option<serde_json::Value>,
        error: Option<String>,
    ) {
        let req = PluginRequest {
            method: protocol::METHOD_IPC_RESULT.to_string(),
            params: serde_json::to_value(IpcCallResult {
                call_id,
                result,
                error,
            })
            .unwrap_or(serde_json::Value::Null),
            id: self.next_request_id.fetch_add(1, Ordering::Relaxed),
        };
        if let Some(proc) = self.processes.get(plugin_id) {
            if let Err(e) = proc.req_tx.send(req) {
                tracing::warn!("plugin {plugin_id}: failed to send ipc.result: {e}");
            }
        }
    }

    /// register_remote_kind에 전달하는 채널 sender. 내부적으로 hello 처리에서
    /// 자체 사용하므로 외부 caller가 직접 쓰지는 않지만, 통합 테스트에서 surface 등록을
    /// 흉내낼 때 노출 필요.
    #[allow(dead_code)]
    pub fn host_cmd_sender(&self) -> Sender<HostCmd> {
        self.host_cmd_tx.clone()
    }

    pub fn set_surface_registry(&mut self, registry: Arc<SurfaceKindRegistry>) {
        self.surface_registry = Some(registry);
    }

    /// 디스커버리 + 활성 plugin 모두 spawn. listener도 여기서 한 번만 bind.
    /// plugin이 없으면 listener 자체를 만들지 않음 (포트 점유 회피).
    pub fn discover_and_start(&mut self) {
        self.packages = crate::plugin::discovery::discover();

        // command registry에 모든 발견된 plugin의 commands를 등록.
        // disabled 여부와 무관 — 설정 UI는 비활성 plugin도 단축키 항목을
        // 보여줘야 사용자가 미리 키를 잡아둘 수 있다.
        self.command_registry =
            super::command_registry::PluginCommandRegistry::new();
        for pkg in &self.packages {
            self.command_registry.register_plugin(&pkg.manifest);
            // i18n namespace 등록 — 비활성 plugin도 설정 UI에서 command title을
            // 번역해서 보여줘야 하므로 disabled 여부와 무관하게 등록한다.
            let lang_dir = pkg.dir.join(&pkg.manifest.lang_dir);
            tasty_core::i18n::register_namespace(&pkg.manifest.id, &lang_dir);
        }

        let to_start: Vec<String> = self
            .packages
            .iter()
            .filter(|p| !self.config.is_disabled(&p.manifest.id))
            .map(|p| p.manifest.id.clone())
            .collect();
        if to_start.is_empty() {
            tracing::info!(
                "plugin: discovered {} package(s), 0 enabled — skipping listener bind",
                self.packages.len()
            );
            return;
        }
        self.ensure_listener();
        for id in &to_start {
            if let Some(pkg) = self.packages.iter().find(|p| &p.manifest.id == id).cloned() {
                self.start_plugin_internal(&pkg);
            }
        }
    }

    fn ensure_listener(&mut self) {
        if self.listener.is_some() {
            return;
        }
        match HostListener::bind() {
            Ok(l) => {
                tracing::info!("plugin host listener on 127.0.0.1:{}", l.port());
                self.listener = Some(l);
            }
            Err(e) => {
                tracing::error!("plugin host listener bind failed: {e}");
            }
        }
    }

    fn start_plugin_internal(&mut self, pkg: &PluginPackage) {
        if self.auto_disabled.contains(&pkg.manifest.id) {
            return;
        }
        let listener = match &self.listener {
            Some(l) => l,
            None => {
                tracing::warn!(
                    "plugin '{}' start skipped — no listener",
                    pkg.manifest.id
                );
                return;
            }
        };
        match PluginProcess::spawn(pkg, listener, &self.log_dir, self.waker.clone()) {
            Ok(p) => {
                tracing::info!("plugin started: {}", p.plugin_id);
                self.processes.insert(pkg.manifest.id.clone(), p);
                self.spawn_failures.remove(&pkg.manifest.id);
                // manifest의 ipc_namespace contribute를 registry에 흡수.
                for ns in &pkg.manifest.contributes.ipc_namespace {
                    if let Err(e) =
                        self.ipc_namespaces.register(&pkg.manifest.id, &ns.prefix)
                    {
                        tracing::warn!(
                            "plugin '{}' ipc namespace registration failed: {}",
                            pkg.manifest.id,
                            e
                        );
                    }
                }
                // surface_observer 구독 등록 (HashSet으로 dedupe).
                for decl in &pkg.manifest.contributes.surface_observer {
                    self.surface_observers
                        .entry(decl.event)
                        .or_default()
                        .insert(pkg.manifest.id.clone());
                }
            }
            Err(e) => {
                tracing::error!("plugin '{}' spawn failed: {}", pkg.manifest.id, e);
                self.record_spawn_failure(&pkg.manifest.id);
            }
        }
    }

    fn record_spawn_failure(&mut self, plugin_id: &str) {
        let now = Instant::now();
        let entry = self
            .spawn_failures
            .entry(plugin_id.to_string())
            .or_default();
        entry.retain(|t| now.duration_since(*t) < RESTART_FAILURE_WINDOW);
        entry.push(now);
        if entry.len() >= RESTART_FAILURE_LIMIT {
            tracing::error!(
                "plugin '{plugin_id}' failed {} times in {}s — auto-disabling until manual re-enable",
                entry.len(),
                RESTART_FAILURE_WINDOW.as_secs()
            );
            self.auto_disabled.insert(plugin_id.to_string());
            self.spawn_failures.remove(plugin_id);
        }
    }

    /// 메인 루프에서 매 tick 호출. plugin 알림 처리 + 헬스체크 + 비응답 재시작.
    pub fn pump(&mut self) {
        // 1. plugin → 호스트 이벤트 처리
        let mut hello_log: Vec<(String, String)> = Vec::new();
        let mut to_register: Vec<String> = Vec::new();
        let mut new_calls: Vec<PendingPluginCall> = Vec::new();
        for (id, proc) in &self.processes {
            while let Ok(ev) = proc.event_rx.try_recv() {
                match ev {
                    PluginEvent::Hello {
                        plugin_id,
                        version,
                    } => {
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
                }
            }
        }
        if !new_calls.is_empty() {
            self.pending_plugin_calls.extend(new_calls);
        }
        for (plugin_id, version) in hello_log {
            tracing::info!("plugin hello: {} v{}", plugin_id, version);
        }
        // hello를 처음 받은 plugin의 surface_kinds를 registry에 등록 + 권한 set 초기화.
        if !to_register.is_empty() {
            // 권한 — registry 유무와 무관하게 항상 갱신.
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
                }
            }
            if let Some(registry) = self.surface_registry.clone() {
                let tx = self.host_cmd_tx.clone();
                for plugin_id in &to_register {
                    if let Some(pkg) =
                        self.packages.iter().find(|p| &p.manifest.id == plugin_id)
                    {
                        for decl in &pkg.manifest.surface_kinds {
                            match decl.rendering {
                                crate::plugin::manifest::SurfaceKindRendering::Remote => {
                                    crate::plugin::remote_kind::register_remote_kind(
                                        &registry,
                                        plugin_id,
                                        decl,
                                        tx.clone(),
                                    );
                                }
                                crate::plugin::manifest::SurfaceKindRendering::Host => {
                                    crate::plugin::host_rendered_kind::register_host_rendered_kind(
                                        &registry, plugin_id, &decl.kind,
                                    );
                                }
                            }
                        }
                    }
                    self.registered_plugins.insert(plugin_id.clone());
                }
            } else {
                tracing::debug!(
                    "plugin manager has no surface_registry; deferring registration of {} plugin(s)",
                    to_register.len()
                );
            }
        }

        // 2. 새로 만들어진 RemoteSurface 등록 + plugin에 surface.create/restore 송신.
        self.drain_host_cmds();

        // 3. RemoteSurface가 모은 사용자 이벤트 → plugin에 surface.event 송신.
        self.flush_pending_events();

        // 4. plugin → 호스트 응답 처리 (tree 동기화).
        self.drain_plugin_responses();

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
            if let Some(proc) = self.processes.remove(&id) {
                proc.shutdown(Duration::from_secs(2));
            }
            self.ipc_namespaces.unregister_plugin(&id);
            self.unregister_observers(&id);
            self.cancel_pending_namespace_calls(&id, "plugin restarting");
            if let Some(pkg) = self.packages.iter().find(|p| p.manifest.id == id).cloned() {
                self.start_plugin_internal(&pkg);
            }
        }
    }

    /// plugin이 종료/재시작될 때 surface_observers 구독 정리.
    /// start_plugin_internal에서 다시 구독되므로 재시작 시 일관성 유지.
    fn unregister_observers(&mut self, plugin_id: &str) {
        for subs in self.surface_observers.values_mut() {
            subs.remove(plugin_id);
        }
        // 빈 set은 제거해 다음 dispatch에서 빈 순회를 피한다.
        self.surface_observers.retain(|_, subs| !subs.is_empty());
    }

    fn drain_host_cmds(&mut self) {
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
                            kind: kind.clone(),
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
                            kind: kind.clone(),
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

    fn flush_pending_events(&mut self) {
        let surface_ids: Vec<u32> = self.surfaces.keys().copied().collect();
        for sid in surface_ids {
            let (plugin_id, events) = match self.surfaces.get(&sid) {
                Some(entry) => {
                    let events = entry
                        .handles
                        .pending_events
                        .lock()
                        .map(|mut v| std::mem::take(&mut *v))
                        .unwrap_or_default();
                    (entry.plugin_id.clone(), events)
                }
                None => continue,
            };
            for ev in events {
                self.send_surface_request(
                    &plugin_id,
                    protocol::METHOD_SURFACE_EVENT,
                    json!({
                        "surface_id": sid,
                        "event": ev,
                    }),
                    PendingRequestKind::SurfaceEvent { surface_id: sid },
                );
            }
        }
    }

    /// 호스트 IPC dispatcher가 받은 namespace 메서드를 owner plugin에 forward.
    /// 응답이 도착하면 `response_tx`로 client에 회신된다. 매칭이 없거나 plugin이
    /// 실행 중이 아니면 즉시 에러 응답을 `response_tx`로 송신.
    ///
    /// caller plugin이 target plugin과 같으면 self-deadlock 위험이 있어 거부한다.
    pub fn forward_namespace_call(
        &mut self,
        method: &str,
        params: serde_json::Value,
        caller_plugin_id: Option<&str>,
        original_id: serde_json::Value,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
    ) {
        let plugin_id = match self.validate_namespace_call(method, caller_plugin_id) {
            Ok(id) => id,
            Err((code, msg)) => {
                let _ = response_tx.send(JsonRpcResponse::error(original_id, code, &msg));
                return;
            }
        };
        let req_id = match self.send_namespace_invoke(&plugin_id, method, &params, caller_plugin_id)
        {
            Ok(id) => id,
            Err(msg) => {
                let _ =
                    response_tx.send(JsonRpcResponse::error(original_id, -32003, &msg));
                return;
            }
        };
        self.pending_requests.insert(
            req_id,
            PendingRequestKind::NamespaceInvoke {
                plugin_id,
                response_tx,
                original_id,
            },
        );
    }

    /// plugin이 보낸 IpcCall이 namespace 메서드인 경우의 forward.
    /// 응답은 caller plugin에 `ipc.result`로 회신한다 (`process_plugin_ipc_calls`와
    /// 같은 방식).
    pub fn forward_namespace_call_from_plugin(
        &mut self,
        method: &str,
        params: serde_json::Value,
        caller_plugin_id: &str,
        call_id: u64,
    ) {
        let plugin_id = match self.validate_namespace_call(method, Some(caller_plugin_id)) {
            Ok(id) => id,
            Err((_code, msg)) => {
                self.send_ipc_result(caller_plugin_id, call_id, None, Some(msg));
                return;
            }
        };
        let req_id =
            match self.send_namespace_invoke(&plugin_id, method, &params, Some(caller_plugin_id)) {
                Ok(id) => id,
                Err(msg) => {
                    self.send_ipc_result(caller_plugin_id, call_id, None, Some(msg));
                    return;
                }
            };
        self.pending_requests.insert(
            req_id,
            PendingRequestKind::PluginToPluginNamespace {
                plugin_id,
                caller_plugin_id: caller_plugin_id.to_string(),
                call_id,
            },
        );
    }

    /// namespace 메서드 호출의 유효성 검사. 성공 시 target plugin id를 반환.
    /// 실패 시 (JSON-RPC code, message) 페어를 반환.
    fn validate_namespace_call(
        &self,
        method: &str,
        caller_plugin_id: Option<&str>,
    ) -> Result<String, (i32, String)> {
        let plugin_id = self
            .ipc_namespaces
            .resolve(method)
            .map(str::to_string)
            .ok_or_else(|| (-32601, format!("method '{method}' not found")))?;
        if let Some(caller) = caller_plugin_id {
            if caller == plugin_id {
                return Err((
                    -32001,
                    format!(
                        "plugin '{caller}' cannot invoke its own namespace method '{method}'"
                    ),
                ));
            }
            let prefix = method.split('.').next().unwrap_or("");
            let required = Permission::IpcInvoke(prefix.to_string());
            let allowed = self
                .plugin_permissions
                .get(caller)
                .map(|set| set.contains(&required))
                .unwrap_or(false);
            if !allowed {
                return Err((
                    -32001,
                    format!(
                        "permission_denied: plugin '{caller}' lacks 'ipc.invoke:{prefix}' \
                         permission for namespace method '{method}'"
                    ),
                ));
            }
        }
        if !self.processes.contains_key(&plugin_id) {
            return Err((
                -32002,
                format!("plugin '{plugin_id}' is not running"),
            ));
        }
        Ok(plugin_id)
    }

    /// target plugin에 `ipc.invoke` 요청을 송신한다. 성공 시 발급된 host→plugin
    /// request id를 반환. caller는 pending_requests에 추적 kind를 직접 삽입한다.
    fn send_namespace_invoke(
        &self,
        plugin_id: &str,
        method: &str,
        params: &serde_json::Value,
        caller_plugin_id: Option<&str>,
    ) -> Result<u64, String> {
        let proc = self
            .processes
            .get(plugin_id)
            .ok_or_else(|| format!("plugin '{plugin_id}' is not running"))?;
        let req_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let req = PluginRequest {
            method: tasty_plugin_protocol::ipc_method::METHOD_IPC_INVOKE.to_string(),
            params: json!({
                "method": method,
                "params": params,
                "caller_plugin_id": caller_plugin_id,
            }),
            id: req_id,
        };
        proc.req_tx
            .send(req)
            .map_err(|e| format!("plugin '{plugin_id}' send failed: {e}"))?;
        Ok(req_id)
    }

    /// 죽거나 비활성화된 plugin이 가진 모든 namespace pending에 에러 응답을 보내고
    /// pending에서 제거한다. CLI/Local caller에게는 response_tx로, plugin caller에게는
    /// `ipc.result`로 회신한다.
    fn cancel_pending_namespace_calls(&mut self, plugin_id: &str, reason: &str) {
        let to_cancel: Vec<u64> = self
            .pending_requests
            .iter()
            .filter_map(|(id, kind)| match kind {
                PendingRequestKind::NamespaceInvoke {
                    plugin_id: pid, ..
                }
                | PendingRequestKind::PluginToPluginNamespace {
                    plugin_id: pid, ..
                } if pid == plugin_id => Some(*id),
                _ => None,
            })
            .collect();
        for id in to_cancel {
            match self.pending_requests.remove(&id) {
                Some(PendingRequestKind::NamespaceInvoke {
                    response_tx,
                    original_id,
                    ..
                }) => {
                    let _ = response_tx.send(JsonRpcResponse::error(
                        original_id,
                        -32004,
                        &format!("plugin '{plugin_id}' unavailable: {reason}"),
                    ));
                }
                Some(PendingRequestKind::PluginToPluginNamespace {
                    caller_plugin_id,
                    call_id,
                    ..
                }) => {
                    self.send_ipc_result(
                        &caller_plugin_id,
                        call_id,
                        None,
                        Some(format!("plugin '{plugin_id}' unavailable: {reason}")),
                    );
                }
                _ => {}
            }
        }
    }

    fn drain_plugin_responses(&mut self) {
        let plugin_ids: Vec<String> = self.processes.keys().cloned().collect();
        for plugin_id in plugin_ids {
            // Drain all responses without holding a borrow on `self.processes`.
            let mut responses: Vec<PluginResponse> = Vec::new();
            if let Some(proc) = self.processes.get(&plugin_id) {
                while let Ok(resp) = proc.resp_rx.try_recv() {
                    responses.push(resp);
                }
            }
            for resp in responses {
                self.handle_plugin_response(&plugin_id, resp);
            }
        }
    }

    fn handle_plugin_response(&mut self, plugin_id: &str, resp: PluginResponse) {
        let kind = self.pending_requests.remove(&resp.id);
        if let Some(err) = &resp.error {
            tracing::warn!("plugin '{plugin_id}' response error (id={}): {err}", resp.id);
        }
        let kind = match kind {
            Some(k) => k,
            None => return,
        };
        match kind {
            PendingRequestKind::SurfaceCreate { surface_id }
            | PendingRequestKind::SurfaceEvent { surface_id }
            | PendingRequestKind::SurfaceRestore { surface_id }
            | PendingRequestKind::CommandInvoke { surface_id } => {
                let result_value = match resp.result {
                    Some(v) => v,
                    None => return,
                };
                let parsed: SurfaceResult = match serde_json::from_value(result_value) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(
                            "plugin '{plugin_id}' surface response decode error: {e}"
                        );
                        return;
                    }
                };
                if let Some(entry) = self.surfaces.get(&surface_id) {
                    if let Some(tree) = parsed.tree {
                        if let Ok(mut slot) = entry.handles.tree.lock() {
                            *slot = Some(tree);
                        }
                    }
                    if let Some(name) = parsed.display_name {
                        if let Ok(mut slot) = entry.handles.display_name.lock() {
                            *slot = name;
                        }
                    }
                }
            }
            PendingRequestKind::Ping | PendingRequestKind::Other => {}
            PendingRequestKind::NamespaceInvoke {
                plugin_id: _,
                response_tx,
                original_id,
            } => {
                let response = if let Some(err) = resp.error {
                    let code = resp.error_code.unwrap_or(-32000);
                    JsonRpcResponse::error(original_id, code, &err)
                } else {
                    JsonRpcResponse::success(
                        original_id,
                        resp.result.unwrap_or(serde_json::Value::Null),
                    )
                };
                let _ = response_tx.send(response);
            }
            PendingRequestKind::PluginToPluginNamespace {
                plugin_id: _,
                caller_plugin_id,
                call_id,
            } => {
                // plugin caller에는 ipc.result로 회신. error_code는 plugin 측이
                // 받은 표준 form 그대로(메시지에 합쳐서 전달).
                self.send_ipc_result(&caller_plugin_id, call_id, resp.result, resp.error);
            }
            PendingRequestKind::SurfaceLifecycle { surface_id } => {
                // fire-and-forget — 응답 본문은 폐기. 에러만 trace 레벨로 기록.
                if let Some(err) = resp.error {
                    tracing::trace!(
                        "plugin '{plugin_id}' surface.lifecycle(surface={surface_id}) returned error: {err}"
                    );
                }
            }
        }
    }

    /// surface 닫힘을 구독 plugin들에게 broadcast. fire-and-forget — 응답 무시.
    /// 호출자(close 경로)가 `reason`을 부여한다.
    pub fn notify_surface_closed(
        &mut self,
        surface_id: u32,
        kind: &str,
        reason: SurfaceCloseReason,
    ) {
        let subs = match self.surface_observers.get(&ObserverEvent::Closed) {
            Some(s) if !s.is_empty() => s.clone(),
            _ => return,
        };
        let params = json!({
            "event": SurfaceLifecycleEvent::Closed,
            "surface_id": surface_id,
            "kind": kind,
            "reason": reason,
        });
        for plugin_id in subs {
            self.send_surface_request(
                &plugin_id,
                protocol::METHOD_SURFACE_LIFECYCLE,
                params.clone(),
                PendingRequestKind::SurfaceLifecycle { surface_id },
            );
        }
    }

    fn send_surface_request(
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

    /// 종료 시 모든 plugin graceful shutdown.
    pub fn shutdown_all(&mut self) {
        for (_, proc) in self.processes.drain() {
            proc.shutdown(Duration::from_secs(2));
        }
    }

    /// CLI/IPC용 — plugin 활성화. 활성화 즉시 spawn 시도.
    pub fn enable(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        self.config.enable(plugin_id);
        self.config.save()?;
        self.auto_disabled.remove(plugin_id);
        if !self.processes.contains_key(plugin_id) {
            self.ensure_listener();
            if let Some(pkg) = self
                .packages
                .iter()
                .find(|p| p.manifest.id == plugin_id)
                .cloned()
            {
                self.start_plugin_internal(&pkg);
            }
        }
        Ok(())
    }

    /// CLI/IPC용 — plugin 비활성화. 살아있는 process는 graceful shutdown.
    pub fn disable(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        self.config.disable(plugin_id);
        self.config.save()?;
        if let Some(proc) = self.processes.remove(plugin_id) {
            proc.shutdown(Duration::from_secs(2));
        }
        self.ipc_namespaces.unregister_plugin(plugin_id);
        self.unregister_observers(plugin_id);
        self.cancel_pending_namespace_calls(plugin_id, "plugin disabled");
        Ok(())
    }

    pub fn is_running(&self, plugin_id: &str) -> bool {
        self.processes.contains_key(plugin_id)
    }

    pub fn log_path(&self, plugin_id: &str) -> PathBuf {
        self.log_dir.join(format!("{plugin_id}.log"))
    }

    /// 호스트 listener의 포트. 디버깅·테스트용.
    #[allow(dead_code)]
    pub fn listener_port(&self) -> Option<u16> {
        self.listener.as_ref().map(|l| l.port())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_waker() -> tasty_core::SharedWakerFactory {
        // headless 환경에서 PluginManager가 사용하는 waker — 실제 wake는 no-op로 충분.
        Arc::new(tasty_core::waker::NoopWakerFactory)
    }

    /// validate_namespace_call의 분기를 직접 검증하기 위한 mgr 초기화.
    /// process는 spawn하지 않고 ipc_namespaces와 plugin_permissions만 직접 채운다.
    fn mgr_with_namespace_owner(owner: &str, prefix: &str) -> PluginManager {
        let mut mgr = PluginManager::new(empty_waker());
        mgr.ipc_namespaces
            .register(owner, prefix)
            .expect("test prefix should be unique");
        mgr
    }

    #[test]
    fn validate_namespace_call_method_not_found() {
        let mgr = PluginManager::new(empty_waker());
        let err = mgr
            .validate_namespace_call("nope.method", None)
            .unwrap_err();
        assert_eq!(err.0, -32601);
    }

    #[test]
    fn validate_namespace_call_local_caller_allowed_when_target_running() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        // process 가짜 entry 삽입 — request 전송은 안 한다, 검증만.
        mgr.processes.insert(
            "com.example.codex".into(),
            stub_process(),
        );
        let id = mgr
            .validate_namespace_call("codex.spawn", None)
            .expect("local caller should pass");
        assert_eq!(id, "com.example.codex");
    }

    #[test]
    fn validate_namespace_call_target_not_running() {
        let mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        let err = mgr
            .validate_namespace_call("codex.spawn", None)
            .unwrap_err();
        assert_eq!(err.0, -32002);
    }

    #[test]
    fn validate_namespace_call_self_invocation_rejected() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes.insert("com.example.codex".into(), stub_process());
        let err = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.codex"))
            .unwrap_err();
        assert_eq!(err.0, -32001);
        assert!(err.1.contains("its own namespace"));
    }

    #[test]
    fn validate_namespace_call_plugin_caller_without_grant_denied() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes.insert("com.example.codex".into(), stub_process());
        // caller plugin은 ipc.invoke:codex 권한 없음
        mgr.set_plugin_permissions(
            "com.example.helper",
            HashSet::from([Permission::SurfaceRead]),
        );
        let err = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.helper"))
            .unwrap_err();
        assert_eq!(err.0, -32001);
        assert!(err.1.contains("permission_denied"));
        assert!(err.1.contains("ipc.invoke:codex"));
    }

    #[test]
    fn validate_namespace_call_plugin_caller_with_grant_allowed() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes.insert("com.example.codex".into(), stub_process());
        mgr.set_plugin_permissions(
            "com.example.helper",
            HashSet::from([Permission::IpcInvoke("codex".into())]),
        );
        let id = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.helper"))
            .expect("granted caller should pass");
        assert_eq!(id, "com.example.codex");
    }

    /// validate 만 보는 테스트용 stub. PluginProcess는 process.rs의 cfg(test)
    /// 헬퍼를 위임 호출한다.
    fn stub_process() -> PluginProcess {
        PluginProcess::stub_for_test("stub")
    }

    #[test]
    fn unregister_observers_removes_plugin_and_cleans_empty_sets() {
        let mut mgr = PluginManager::new(empty_waker());
        mgr.surface_observers
            .entry(ObserverEvent::Closed)
            .or_default()
            .insert("com.example.a".to_string());
        mgr.surface_observers
            .entry(ObserverEvent::Closed)
            .or_default()
            .insert("com.example.b".to_string());

        mgr.unregister_observers("com.example.a");
        let subs = mgr
            .surface_observers
            .get(&ObserverEvent::Closed)
            .expect("event still has subscribers");
        assert_eq!(subs.len(), 1);
        assert!(subs.contains("com.example.b"));

        mgr.unregister_observers("com.example.b");
        // 빈 set은 retain으로 제거되어야 한다.
        assert!(!mgr.surface_observers.contains_key(&ObserverEvent::Closed));
    }

    #[test]
    fn notify_surface_closed_without_subscribers_is_noop() {
        let mut mgr = PluginManager::new(empty_waker());
        let before = mgr.pending_requests.len();
        mgr.notify_surface_closed(7, "terminal", SurfaceCloseReason::UserClose);
        // 구독자가 없으면 dispatch 자체를 시도하지 않음 → pending_requests 변화 없음.
        assert_eq!(mgr.pending_requests.len(), before);
    }

    #[test]
    fn notify_surface_closed_skips_when_target_process_missing() {
        // 구독은 등록되어 있지만 plugin process가 죽어있는 경우 — send_surface_request가
        // processes.get(plugin_id).is_none() → 조용히 무시.
        let mut mgr = PluginManager::new(empty_waker());
        mgr.surface_observers
            .entry(ObserverEvent::Closed)
            .or_default()
            .insert("com.example.ghost".to_string());
        let before = mgr.pending_requests.len();
        mgr.notify_surface_closed(7, "terminal", SurfaceCloseReason::AgentClose);
        assert_eq!(mgr.pending_requests.len(), before);
    }
}
