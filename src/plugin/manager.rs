//! Plugin 생명주기 매니저.
//!
//! 호스트의 부팅 시 한 번 만들어지고, `App`이 유일한 인스턴스를 보유한다.
//! - 부팅 시 `discover_and_start()`로 `~/.tasty/plugins/`를 스캔하여 활성 plugin 모두 spawn
//! - 매 메인 루프 tick에서 `pump()` 호출 → plugin 알림 처리 + 헬스체크 + 재시작
//! - 종료 시 `shutdown_all()`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::plugin::listener::HostListener;
use crate::plugin::manifest::PluginPackage;
use crate::plugin::process::PluginProcess;
use crate::plugin::protocol::PluginEvent;
use crate::plugin::registry_state::PluginsConfig;
use crate::surface_registry::SurfaceKindRegistry;

const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(60);
const PING_INTERVAL: Duration = Duration::from_secs(15);
const RESTART_FAILURE_WINDOW: Duration = Duration::from_secs(10);
const RESTART_FAILURE_LIMIT: usize = 3;

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
}

impl PluginManager {
    pub fn new(waker: tasty_core::SharedWakerFactory) -> Self {
        let log_dir = tasty_core::paths::tasty_home()
            .map(|d| d.join("plugins-logs"))
            .unwrap_or_else(|| PathBuf::from("./plugin-logs"));
        let _ = std::fs::create_dir_all(&log_dir);
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
        }
    }

    pub fn set_surface_registry(&mut self, registry: Arc<SurfaceKindRegistry>) {
        self.surface_registry = Some(registry);
    }

    /// 디스커버리 + 활성 plugin 모두 spawn. listener도 여기서 한 번만 bind.
    /// plugin이 없으면 listener 자체를 만들지 않음 (포트 점유 회피).
    pub fn discover_and_start(&mut self) {
        self.packages = crate::plugin::discovery::discover();
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
                }
            }
        }
        for (plugin_id, version) in hello_log {
            tracing::info!("plugin hello: {} v{}", plugin_id, version);
        }
        // hello를 처음 받은 plugin의 surface_kinds를 registry에 등록.
        if !to_register.is_empty() {
            if let Some(registry) = self.surface_registry.clone() {
                for plugin_id in &to_register {
                    if let Some(pkg) =
                        self.packages.iter().find(|p| &p.manifest.id == plugin_id)
                    {
                        for decl in &pkg.manifest.surface_kinds {
                            crate::plugin::remote_kind::register_remote_kind(
                                &registry, plugin_id, decl,
                            );
                        }
                    }
                    self.registered_plugins.insert(plugin_id.clone());
                }
            } else {
                // registry 미설정 — 등록 보류 (다음에 set_surface_registry 후 재시도 가능)
                tracing::debug!(
                    "plugin manager has no surface_registry; deferring registration of {} plugin(s)",
                    to_register.len()
                );
            }
        }

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
            if let Some(pkg) = self.packages.iter().find(|p| p.manifest.id == id).cloned() {
                self.start_plugin_internal(&pkg);
            }
        }
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
        Ok(())
    }

    pub fn is_running(&self, plugin_id: &str) -> bool {
        self.processes.contains_key(plugin_id)
    }

    pub fn log_path(&self, plugin_id: &str) -> PathBuf {
        self.log_dir.join(format!("{plugin_id}.log"))
    }

    pub fn listener_port(&self) -> Option<u16> {
        self.listener.as_ref().map(|l| l.port())
    }
}
