//! Plugin 생명주기: 인스턴스 생성, listener bind, discover→spawn, healthcheck restart 후
//! plugin process 정리, enable/disable, 권한 갱신, log path 조회.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tasty_plugin_protocol::host_port::SurfaceRegistry;

use crate::plugin::handle_channel::HandleListener;
use crate::plugin::ipc_namespace::IpcNamespaceRegistry;
use crate::plugin::listener::HostListener;
use crate::plugin::manifest::{Permission, PluginPackage};
use crate::plugin::process::PluginProcess;
use crate::plugin::registry_state::PluginsConfig;

use super::{PluginManager, RESTART_FAILURE_LIMIT, RESTART_FAILURE_WINDOW};

impl PluginManager {
    /// 기본 file_format/file_handler 레지스트리를 새로 발급해서 초기화.
    /// production 경로는 `App` 가 공유 Arc 를 갖고 있어 `with_registries` 를
    /// 직접 호출 — 본 ctor 는 내부 unit test 전용.
    #[cfg(test)]
    pub fn new(waker: tasty_terminal::waker_factory::SharedWakerFactory) -> Self {
        Self::with_registries(
            waker,
            Arc::new(crate::file::format::FileFormatRegistry::new()),
            Arc::new(crate::file::handler::FileHandlerRegistry::new()),
        )
    }

    /// CoreState 와 같은 Arc 를 공유하기 위한 생성자.
    pub fn with_registries(
        waker: tasty_terminal::waker_factory::SharedWakerFactory,
        file_format: Arc<dyn tasty_plugin_protocol::host_port::FileFormatRegistryPort>,
        file_handler: Arc<dyn tasty_plugin_protocol::host_port::FileHandlerRegistryPort>,
    ) -> Self {
        let log_dir = tasty_utils::path::tasty_home()
            .map(|d| d.join("plugins-logs"))
            .unwrap_or_else(|| PathBuf::from("./plugin-logs"));
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            tracing::warn!("plugin log dir {} create failed: {e}", log_dir.display());
        }
        let (host_cmd_tx, host_cmd_rx) = mpsc::channel();
        Self {
            packages: Vec::new(),
            processes: HashMap::new(),
            config: PluginsConfig::load(),
            waker,
            listener: None,
            handle_listener: None,
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
            command_registry: crate::plugin::command_registry::PluginCommandRegistry::new(),
            ipc_namespaces: IpcNamespaceRegistry::new(),
            plugin_buffers: HashMap::new(),
            next_buffer_id: AtomicU64::new(1),
            extensions: crate::plugin::extension_registry::ExtensionRegistry::new(),
            hook_failures: HashMap::new(),
            event_bus: crate::plugin::event_bus::EventBus::new(),
            event_trace_seq: AtomicU64::new(1),
            popup_instances: HashMap::new(),
            next_popup_instance_id: 1,
            file_format,
            file_handler,
            i18n_registrar: None,
        }
    }

    /// 호스트가 i18n namespace 등록 trait 을 주입. headless/test 는 호출 안 함.
    pub fn set_i18n_registrar(
        &mut self,
        registrar: Arc<dyn tasty_plugin_protocol::host_port::I18nNamespaceRegistrar>,
    ) {
        self.i18n_registrar = Some(registrar);
    }

    /// plugin에 grant된 권한 set을 갱신. 매니페스트 hello 시점 또는 사용자가
    /// grant/revoke 했을 때 호출. plugin process 재시작 없이 즉시 반영된다.
    pub fn set_plugin_permissions(&mut self, plugin_id: &str, perms: HashSet<Permission>) {
        self.plugin_permissions
            .insert(plugin_id.to_string(), Arc::new(perms));
    }

    /// 호스트 main loop이 라우팅하기 위해 plugin IPC 호출을 모두 가져간다.
    pub fn set_surface_registry(&mut self, registry: Arc<dyn SurfaceRegistry>) {
        self.surface_registry = Some(registry);
    }

    /// 디스커버리 + 활성 plugin 모두 spawn. listener도 여기서 한 번만 bind.
    /// plugin이 없으면 listener 자체를 만들지 않음 (포트 점유 회피).
    pub fn discover_and_start(&mut self) {
        self.packages = crate::plugin::discovery::discover();

        // command registry에 모든 발견된 plugin의 commands를 등록.
        // disabled 여부와 무관 — 설정 UI는 비활성 plugin도 단축키 항목을
        // 보여줘야 사용자가 미리 키를 잡아둘 수 있다.
        self.command_registry = crate::plugin::command_registry::PluginCommandRegistry::new();
        for pkg in &self.packages {
            self.command_registry.register_plugin(&pkg.manifest);
            // i18n namespace 등록 — 비활성 plugin도 설정 UI에서 command title을
            // 번역해서 보여줘야 하므로 disabled 여부와 무관하게 등록한다.
            let lang_dir = pkg.dir.join(&pkg.manifest.lang_dir);
            if let Some(reg) = &self.i18n_registrar {
                reg.register(&pkg.manifest.id, &lang_dir);
            }
        }

        self.recompute_extensions();

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
        if self.listener.is_none() {
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
        if self.handle_listener.is_none() {
            match HandleListener::bind() {
                Ok(l) => {
                    tracing::info!("plugin handle channel listener at {}", l.endpoint());
                    self.handle_listener = Some(l);
                }
                Err(e) => {
                    // 보조 채널 없이도 plugin 본 기능은 동작. shared buffer를 쓰는 plugin만
                    // 이후 핸드셰이크 단계에서 실패.
                    tracing::warn!("plugin handle channel listener bind failed: {e}");
                }
            }
        }
    }

    pub(super) fn start_plugin_internal(&mut self, pkg: &PluginPackage) {
        if self.auto_disabled.contains(&pkg.manifest.id) {
            return;
        }
        let listener = match &self.listener {
            Some(l) => l,
            None => {
                tracing::warn!("plugin '{}' start skipped — no listener", pkg.manifest.id);
                return;
            }
        };
        match PluginProcess::spawn(
            pkg,
            listener,
            self.handle_listener.as_ref(),
            &self.log_dir,
            self.waker.clone(),
        ) {
            Ok(p) => {
                tracing::info!("plugin started: {}", p.plugin_id);
                self.processes.insert(pkg.manifest.id.clone(), p);
                self.spawn_failures.remove(&pkg.manifest.id);
                // `plugin.loaded` 발화 위치 — D.3.C.G.2.e 부터 hello 수신 후 호출자
                // (App::finalize_plugin_hello) 가 cascade 로 발화. spawn-time 직접
                // 발화는 제거 (이중 발화 회피).
                // manifest의 ipc_namespace contribute를 registry에 흡수.
                for ns in &pkg.manifest.contributes.ipc_namespace {
                    if let Err(e) = self.ipc_namespaces.register(&pkg.manifest.id, &ns.prefix) {
                        tracing::warn!(
                            "plugin '{}' ipc namespace registration failed: {}",
                            pkg.manifest.id,
                            e
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!("plugin '{}' spawn failed: {}", pkg.manifest.id, e);
                {
                    use tasty_plugin_protocol::EventScope;
                    use tasty_plugin_protocol::events::payloads::PluginError;
                    let payload = PluginError {
                        plugin_id: pkg.manifest.id.clone(),
                        error_kind: "spawn_failed".to_string(),
                        message: e.to_string(),
                    };
                    self.emit_host_event("plugin.error", &payload, EventScope::System);
                }
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
    pub fn shutdown_all(&mut self) {
        for (_, proc) in self.processes.drain() {
            proc.shutdown(Duration::from_secs(2));
        }
        self.plugin_buffers.clear();
    }

    /// CLI/IPC용 — plugin 활성화. 활성화 즉시 spawn 시도.
    pub fn enable(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        self.config.enable(plugin_id);
        self.config.save()?;
        self.auto_disabled.remove(plugin_id);
        self.recompute_extensions();
        if let Some(pkg) = self
            .packages
            .iter()
            .find(|p| p.manifest.id == plugin_id)
            .cloned()
        {
            // file_format / file_handler 두 registry 에 plugin 의 contribute 등록.
            // plugin process spawn 과 별개로 정적 contribute 는 즉시 활성화한다.
            self.file_format
                .install_plugin_detectors(plugin_id, &pkg.manifest.contributes.detector);
            self.file_handler
                .install_plugin_handlers(plugin_id, &pkg.manifest.contributes.handler);

            if !self.processes.contains_key(plugin_id) {
                self.ensure_listener();
                self.start_plugin_internal(&pkg);
            }
        }
        // `plugin.enabled` 발화는 D.3.C.G.2.b cascade 가 처리 (App::plugin_enable
        // 의 CoreEvent::PluginEnableToggled → cascade).
        Ok(())
    }

    /// CLI/IPC용 — plugin 비활성화. 살아있는 process는 graceful shutdown.
    pub fn disable(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        self.config.disable(plugin_id);
        self.config.save()?;
        self.recompute_extensions();
        let was_running = self.processes.contains_key(plugin_id);
        if let Some(proc) = self.processes.remove(plugin_id) {
            proc.shutdown(Duration::from_secs(2));
        }
        self.ipc_namespaces.unregister_plugin(plugin_id);
        // file_format / file_handler 두 registry 에서 plugin 의 contribute 제거.
        self.file_format.uninstall_plugin(plugin_id);
        self.file_handler.uninstall_plugin(plugin_id);
        // `plugin.unloaded` / `plugin.disabled` 발화는 D.3.C.G.2.b cascade 가 처리
        // (App::plugin_disable 의 CoreEvent::PluginEnableToggled + PluginUnloaded
        // → cascade). was_running 분기는 App::plugin_disable 가 사전 캡처하므로 본
        // 메서드 안에서는 사용 안 함.
        let _ = was_running; // 의도적으로 무시 — 발화는 cascade 가 담당.
        self.event_bus.clear_plugin(plugin_id);
        self.cancel_pending_namespace_calls(plugin_id, "plugin disabled");
        self.plugin_buffers.remove(plugin_id);
        Ok(())
    }

    pub fn is_running(&self, plugin_id: &str) -> bool {
        self.processes.contains_key(plugin_id)
    }

    /// 현재 `packages` + `config.is_disabled`를 기준으로 extension 상태를 재계산.
    /// 디스커버리/enable/disable/install/remove 후 매번 호출한다.
    pub fn log_path(&self, plugin_id: &str) -> PathBuf {
        self.log_dir.join(format!("{plugin_id}.log"))
    }
}
