//! Plugin 생명주기: 인스턴스 생성, listener bind, discover→spawn, healthcheck restart 후
//! plugin process 정리, enable/disable, 권한 갱신, log path 조회.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use std::time::Instant;

use tasty_plugin_protocol::host_port::SurfaceRegistry;

use crate::handle_channel::HandleListener;
use crate::listener::HostListener;
use crate::process::{CHILD_EXIT_POLL_INTERVAL, PluginProcess, ShutdownBatch};
use crate::registry_state::PluginsConfig;
use tasty_ipc::ipc_namespace::IpcNamespaceRegistry;
use tasty_plugin_manifest::{Permission, PluginPackage};

use super::{
    AUTO_RELOAD_POLL_INTERVAL, PING_INTERVAL, PLUGIN_SHUTDOWN_TIMEOUT, PluginManager, PluginTick,
    RESTART_FAILURE_LIMIT, RESTART_FAILURE_WINDOW, RSS_SAMPLE_INTERVAL,
};

/// 완료 판정 전략 레지스트리의 plugin owner 문자열. `[[contributes.
/// completion_strategy]]`의 `poll_method`/`default_for_methods` 는 plugin 의
/// 실제 IPC dispatch 접두어(`[[contributes.ipc_namespace]].prefix`, 예:
/// `"claude"`)로 제한된다(결정 2) — 레지스트리는 owner 문자열과 그 접두어를
/// 그대로 문자열 비교하므로, reverse-DNS 매니페스트 `id`(예:
/// `"com.tasty.claude"`)를 owner 로 넘기면 어떤 실제 poll_method 접두어와도
/// 겹치지 않아 결정 2 가 모든 poll 전략을 무조건 drop 시킨다. namespace 를
/// 선언하지 않은 plugin 은 애초에 poll_method 로 참조할 자기 namespace 가
/// 없으므로 manifest id 로 폴백해도(install/uninstall 양쪽에서 동일 폴백이라
/// 서로 어긋나지 않음) 그 plugin 의 poll 전략은 여전히 (정당하게) 전부 drop된다.
fn completion_strategy_owner_id(pkg: &PluginPackage) -> &str {
    pkg.manifest
        .contributes
        .ipc_namespace
        .first()
        .map(|ns| ns.prefix.as_str())
        .unwrap_or(pkg.manifest.id.as_str())
}

impl PluginManager {
    /// 부팅 시 등록하는 plugin 주기 작업. auto-reload 는 flag 가 켜질 때만 등록된다
    /// ([`PluginManager::set_auto_reload_enabled`]).
    fn initial_timers(now: Instant) -> tasty_timer::TimerHub<PluginTick> {
        let mut hub = tasty_timer::TimerHub::new();
        hub.every(
            PluginTick::Ping,
            PING_INTERVAL,
            tasty_timer::Precision::Strict,
            now,
        );
        // RSS 는 관측용이라 늦어도 무해하다 — 자기 힘으로 호스트를 깨우지 않고
        // ping wakeup 에 편승한다(slack 이 ping 주기라 최악에도 한 ping 만큼만 밀린다).
        hub.every(
            PluginTick::Rss,
            RSS_SAMPLE_INTERVAL,
            tasty_timer::Precision::Lax {
                slack: PING_INTERVAL,
            },
            now,
        );
        hub
    }

    /// auto-reload 를 켜고 끈다. **켤 때만 타이머가 등록된다** — 꺼진 기능이
    /// `next_deadline()` 에 기여하지 않는 것이 이 게이트의 요점이다(개발용 flag 가
    /// production 의 idle wakeup 을 만들지 않는다).
    pub(super) fn set_auto_reload_enabled(&mut self, enabled: bool, now: Instant) {
        self.auto_reload_enabled = enabled;
        if enabled {
            self.timers.every(
                PluginTick::AutoReload,
                AUTO_RELOAD_POLL_INTERVAL,
                tasty_timer::Precision::Strict,
                now,
            );
        } else {
            self.timers.cancel(PluginTick::AutoReload);
        }
    }

    /// 이 매니저가 호스트를 깨우기를 요구하는 가장 가까운 시각. 호스트는 자기
    /// 허브의 데드라인과 `min` 을 취한다(`docs/dev-guide/timer-hub.md`).
    pub fn next_deadline(&self) -> Option<Instant> {
        self.timers.next_deadline()
    }

    /// 관측용 타이머 스냅샷. 호스트가 자기 허브 스냅샷과 합쳐 `timer.list` 로
    /// 노출한다(`docs/dev-guide/timer-hub.md` "관측").
    ///
    /// [`PluginTick`] 은 이 크레이트 내부 어휘라 밖으로 내보내지 않는다 — 대신
    /// 표시용 라벨로 옮긴 뒤 넘긴다. 조회 전용이며 등록/취소 경로는 열지 않는다.
    pub fn timer_snapshot(&self) -> Vec<tasty_timer::TimerSnapshot<&'static str>> {
        self.timers
            .snapshot()
            .into_iter()
            .map(|s| tasty_timer::TimerSnapshot {
                key: match s.key {
                    PluginTick::Ping => "PluginPing",
                    PluginTick::Rss => "PluginRss",
                    PluginTick::AutoReload => "PluginAutoReload",
                },
                interval: s.interval,
                next_due: s.next_due,
                precision: s.precision,
                last_fired: s.last_fired,
            })
            .collect()
    }

    /// 기본 file_format/file_handler stub 으로 초기화 — 내부 unit test 전용.
    /// production 경로는 `App` 가 공유 Arc 를 갖고 있어 `with_registries` 를 직접
    /// 호출. F.B.11-4 이후, host file 도메인 결합 회피를 위해 test ctor 는
    /// no-op stub 으로 변경 — 실제 file_format/handler 검증을 거치는 test 는
    /// 본 바이너리 통합 test 로 이전.
    #[cfg(test)]
    pub fn new(waker: tasty_terminal::waker_factory::SharedWakerFactory) -> Self {
        struct StubFormat;
        impl tasty_plugin_protocol::host_port::FileFormatRegistryPort for StubFormat {
            fn install_plugin_detectors(&self, _: &str, _: &[serde_json::Value]) {}
            fn uninstall_plugin(&self, _: &str) {}
        }
        struct StubHandler;
        impl tasty_plugin_protocol::host_port::FileHandlerRegistryPort for StubHandler {
            fn install_plugin_handlers(&self, _: &str, _: &[serde_json::Value]) {}
            fn uninstall_plugin(&self, _: &str) {}
        }
        Self::with_registries(waker, Arc::new(StubFormat), Arc::new(StubHandler))
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
        // 플러그인 수명 결박 reaper. Windows 는 Job Object 생성을 시도하고, 실패 시
        // 결박 없이 기존 kill 기반 정리로 degrade. 비-Windows 는 무조건 성공(stub).
        let plugin_reaper = crate::reaper::PluginReaper::new().unwrap_or_else(|e| {
            tracing::warn!("plugin reaper init failed — plugin lifetime binding disabled: {e}");
            crate::reaper::PluginReaper::disabled()
        });
        Self {
            packages: Vec::new(),
            rejected: Vec::new(),
            processes: HashMap::new(),
            config: PluginsConfig::load(),
            waker,
            listener: None,
            handle_listener: None,
            log_dir,
            next_request_id: AtomicU64::new(1),
            timers: Self::initial_timers(Instant::now()),
            spawn_failures: HashMap::new(),
            auto_disabled: std::collections::HashSet::new(),
            plugin_binary_mtimes: HashMap::new(),
            plugin_manifest_versions: HashMap::new(),
            auto_reload_enabled: false,
            surface_registry: None,
            registered_plugins: std::collections::HashSet::new(),
            host_cmd_tx,
            host_cmd_rx,
            surfaces: HashMap::new(),
            pending_requests: HashMap::new(),
            plugin_permissions: HashMap::new(),
            pending_plugin_calls: Vec::new(),
            command_registry: crate::command_registry::PluginCommandRegistry::new(),
            settings_pages: crate::settings_registry::SettingsPageRegistry::new(),
            ipc_namespaces: std::sync::Arc::new(
                std::sync::RwLock::new(IpcNamespaceRegistry::new()),
            ),
            plugin_buffers: HashMap::new(),
            next_buffer_id: AtomicU64::new(1),
            egui_mesh_frames: HashMap::new(),
            popup_mesh_frames: HashMap::new(),
            extensions: crate::extension_registry::ExtensionRegistry::new(),
            hook_failures: HashMap::new(),
            event_bus: crate::event_bus::EventBus::new(),
            event_trace_seq: AtomicU64::new(1),
            popup_instances: HashMap::new(),
            next_popup_instance_id: 1,
            banner_instances: HashMap::new(),
            next_banner_instance_id: 1,
            banner_mesh_frames: HashMap::new(),
            invalidated_surfaces: Vec::new(),
            invalidated_popups: Vec::new(),
            sys: sysinfo::System::new(),
            pending_rss_samples: Vec::new(),
            file_format,
            file_handler,
            hook_handler: None,
            completion_strategy: None,
            i18n_registrar: None,
            plugin_reaper,
            shutdown_batch: None,
        }
    }

    /// 호스트가 공유 훅 핸들러 레지스트리 port 를 주입. headless/test 는 호출 안 함.
    /// 주입 후에는 plugin enable/disable 시 `[[contributes.hook_handler]]` 를 등록/해제한다.
    pub fn set_hook_handler_registry(
        &mut self,
        registry: Arc<dyn tasty_plugin_protocol::host_port::HookHandlerRegistryPort>,
    ) {
        self.hook_handler = Some(registry);
    }

    /// 호스트가 완료 판정 전략 레지스트리 port 를 주입. headless/test 는
    /// 호출 안 함. 주입 후에는 plugin enable/disable 시
    /// `[[contributes.completion_strategy]]` 를 등록/해제한다.
    pub fn set_completion_strategy_registry(
        &mut self,
        registry: Arc<dyn tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort>,
    ) {
        self.completion_strategy = Some(registry);
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
    /// `~/.tasty/plugins/` 를 다시 스캔해 `packages` 와 `rejected` 를 함께 갱신.
    /// 모든 discover 호출 지점은 이 헬퍼를 거쳐 거부 목록이 누락되지 않게 한다.
    pub fn refresh_packages(&mut self) {
        let (packages, rejected) = crate::discovery::discover_with_rejections();
        self.packages = packages;
        self.rejected = rejected;
        self.sync_ipc_namespaces_from_packages();
    }

    /// 라우팅 해소의 재료를 **설치 사실**에 맞춘다 — 실행 여부는 여기서 묻지 않는다.
    ///
    /// 그 물음은 같은 호출 경로에서 `validate_namespace_call` 의 `processes` 검사가
    /// 이미 따로 지고 있고(`-32002 plugin '…' is not running`), 두 물음을 표 하나에
    /// 겹쳐 두면 꺼진 plugin 의 메서드가 "그런 메서드 없다" 로 답해 **거짓이 된다.**
    ///
    /// 스캔 시점에 채우는 것이 중요한 이유가 하나 더 있다: 이 표를 spawn 시점에
    /// 채우면 헤드리스는 "이 이름이 plugin 소속인가" 를 **묻기 위해 먼저 plugin 을
    /// 띄워야** 한다. 실측(2026-09-05)에서 그 대가는 오타 한 번당 프로세스 9 개와
    /// 1.2 초였고, 그 프로세스는 데몬 수명 내내 남았다. 근거·수·대안은
    /// [ADR-0173](../../../../docs/adr/0173-namespace-resolution-reads-the-manifest-not-the-process-table.md).
    fn sync_ipc_namespaces_from_packages(&mut self) {
        let fresh = self.freshly_computed_namespaces();
        // **계산은 락 밖에서 끝났다.** 임계구역은 대입 한 줄뿐이라 그 안에서 도는
        // 코드가 없다 — 표를 읽는 쪽(`method_meta`)이 이 락을 다시 잡으러 들어올
        // 자리가 없다는 뜻이다. 항목을 하나씩 등록/해제하면 그 사이사이가 전부
        // 임계구역이 되고, 그때는 재진입이 가능해진다.
        *self.namespaces_write() = fresh;
    }

    /// 설치된 매니페스트만으로 소유 표를 **처음부터** 만든다.
    ///
    /// 낡은 것을 골라 지우고 새 것을 더하는 대신 통째로 다시 만드는 이유는 그것이
    /// 유도의 정의이기 때문이다 — 이 표는 `packages` 의 함수이고 그 밖의 재료가 없다
    /// (ADR-0173). 차분으로 만들면 "어디서 왔는지 모르는 항목" 이 남을 수 있고, 그것이
    /// 바로 제거된 plugin 의 prefix 가 표에 남아 있던 결함의 형태였다.
    fn freshly_computed_namespaces(&self) -> IpcNamespaceRegistry {
        let mut fresh = IpcNamespaceRegistry::new();
        for package in &self.packages {
            let id = &package.manifest.id;
            for ns in &package.manifest.contributes.ipc_namespace {
                if let Err(e) = fresh.register(id, &ns.prefix) {
                    tracing::warn!("plugin '{id}' ipc namespace registration failed: {e}");
                }
            }
        }
        fresh
    }

    /// 설치 목록을 놓고 **유도까지** 돌린다 — 운영에서 `refresh_packages` 가 하는 것과
    /// 같은 순서다. 디스크 스캔만 건너뛴다.
    #[cfg(test)]
    pub(crate) fn set_packages_for_tests(&mut self, packages: Vec<crate::PluginPackage>) {
        self.packages = packages;
        self.sync_ipc_namespaces_from_packages();
    }

    /// 설치 목록만 바꾸고 **유도를 안 돌린다** — 신선도 단정의 대조군 전용이다.
    /// 운영 경로에 이런 자리가 있으면 그것이 곧 이 단정이 잡으려는 결함이다.
    #[cfg(test)]
    pub(crate) fn overwrite_packages_without_deriving_for_tests(
        &mut self,
        packages: Vec<crate::PluginPackage>,
    ) {
        self.packages = packages;
    }

    /// 소유 표가 지금 `packages` 로 다시 계산한 것과 같은가 — **debug 빌드 전용.**
    ///
    /// 텍스트 가드가 못 보는 것이 순서다. "유도를 부르는가" 는 소스에서 보이지만
    /// "유도 **뒤에** 원본을 또 쓰지 않았는가" 는 안 보인다. 그 형태의 결함이 이
    /// 저장소에서 실제로 났다(확장 집합에서 `config.save()` 가 유도 뒤에 있던 것).
    pub fn debug_assert_namespaces_fresh(&self) {
        #[cfg(debug_assertions)]
        {
            let fresh = self.freshly_computed_namespaces();
            assert!(
                fresh == *self.namespaces_read(),
                "namespace 소유 표가 낡았다 — 유도(`refresh_packages`) 뒤에 원본(`packages`)이 \
                 또 바뀌었다. 유도를 원본의 마지막 쓰기 뒤로 옮겨라"
            );
        }
    }

    pub fn discover_and_start(&mut self) {
        // H.b — env flag 한 번 평가. TASTY_PLUGIN_AUTO_RELOAD 가 비어있지 않고
        // "0" 이 아니면 enable. 기본 false (production 부작용 0).
        let enabled = std::env::var("TASTY_PLUGIN_AUTO_RELOAD")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);
        self.set_auto_reload_enabled(enabled, Instant::now());
        if self.auto_reload_enabled {
            tracing::info!("plugin auto-reload: enabled (TASTY_PLUGIN_AUTO_RELOAD)");
        }
        self.refresh_packages();
        self.register_all_package_commands();
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
            self.start_enabled_package(id);
        }
    }

    /// command registry에 모든 발견된 plugin의 commands를 등록.
    /// disabled 여부와 무관 — 설정 UI는 비활성 plugin도 단축키 항목을
    /// 보여줘야 사용자가 미리 키를 잡아둘 수 있다.
    fn register_all_package_commands(&mut self) {
        self.command_registry = crate::command_registry::PluginCommandRegistry::new();
        for pkg in &self.packages {
            self.command_registry.register_plugin(&pkg.manifest);
            // i18n namespace 등록 — 비활성 plugin도 설정 UI에서 command title을
            // 번역해서 보여줘야 하므로 disabled 여부와 무관하게 등록한다.
            let lang_dir = pkg.dir.join(&pkg.manifest.lang_dir);
            if let Some(reg) = &self.i18n_registrar {
                reg.register(&pkg.manifest.id, &lang_dir);
            }
        }
    }

    /// `discover_and_start` 부팅 경로 — enable() 과 대칭으로 정적 contribute 를
    /// 등록 후 spawn 시도. `id` 가 packages 에 없으면(레이스) no-op.
    fn start_enabled_package(&mut self, id: &str) {
        let Some(pkg) = self.packages.iter().find(|p| &p.manifest.id == id).cloned() else {
            return;
        };
        // 부팅 경로에서도 enable() 과 대칭으로 정적 contribute 를 두 registry 에
        // 등록한다. spawn 성공 여부와 무관하게 detector/handler 가 즉시
        // 활성화되도록 start_plugin_internal(spawn) 과 분리해 enabled 판정 직후
        // install. (멱등 — push_contribution 이 같은 owner 를 retain 으로 교체.)
        self.file_format
            .install_plugin_detectors(&pkg.manifest.id, &pkg.manifest.contributes.detector);
        self.file_handler
            .install_plugin_handlers(&pkg.manifest.id, &pkg.manifest.contributes.handler);
        if let Some(hh) = &self.hook_handler {
            hh.install_plugin_hook_handlers(
                &pkg.manifest.id,
                &pkg.manifest.contributes.hook_handler,
            );
        }
        if let Some(cs) = &self.completion_strategy {
            cs.install_plugin_completion_strategies(
                completion_strategy_owner_id(&pkg),
                &pkg.manifest.contributes.completion_strategy,
            );
        }
        self.start_plugin_internal(&pkg);
    }

    fn ensure_listener(&mut self) {
        self.ensure_tcp_listener();
        self.ensure_handle_listener();
    }

    fn ensure_tcp_listener(&mut self) {
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

    fn ensure_handle_listener(&mut self) {
        if self.handle_listener.is_some() {
            return;
        }
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
            &self.plugin_reaper,
        ) {
            Ok(p) => self.on_plugin_spawn_success(pkg, p),
            Err(e) => self.on_plugin_spawn_failure(pkg, e),
        }
    }

    fn on_plugin_spawn_success(&mut self, pkg: &PluginPackage, p: PluginProcess) {
        tracing::info!("plugin started: {}", p.plugin_id);
        self.processes.insert(pkg.manifest.id.clone(), p);
        self.spawn_failures.remove(&pkg.manifest.id);
        // H.b — spawn 성공 분기에서만 baseline 캡처. 무한 swap loop 회피용
        // 기준점. 실패 시 entry 가 디스크에 없거나 metadata 실패해도
        // capture 가 None 으로 끝남 — 다음 check_for_updates 에서 비교 대상
        // 없으면 skip.
        self.capture_plugin_baseline(&pkg.manifest.id);
        // `plugin.loaded` 발화 위치 — D.3.C.G.2.e 부터 hello 수신 후 호출자
        // (App::finalize_plugin_hello) 가 cascade 로 발화. spawn-time 직접
        // 발화는 제거 (이중 발화 회피).
        //
        // ipc namespace 등록은 **여기 없다.** 소유는 설치 사실이지 기동 사실이
        // 아니므로 `sync_ipc_namespaces_from_packages` 가 스캔 시점에 채운다.
    }

    fn on_plugin_spawn_failure(&mut self, pkg: &PluginPackage, e: anyhow::Error) {
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

    /// 호스트 종료 시 전 plugin 프로세스 정리 — **전 plugin 에 shutdown 요청을
    /// 먼저 뿌린 뒤** 대기를 겹친다. plugin 은 서로 독립 프로세스라 대기가 직렬일
    /// 이유가 없다: 총 소요는 Σ(개별 2s) 가 아니라 max(2s) 로 수렴한다.
    ///
    /// 반환 시점에 모든 자식이 회수(exit 관측 또는 kill+wait 완료)돼 있다.
    /// 프레임을 계속 돌려야 하는 호출자는 이 블로킹 형태 대신
    /// [`Self::begin_shutdown_all`] + [`Self::poll_shutdown_all`] 을 직접 조합한다.
    ///
    /// 계측은 `target: "tasty::shutdown"` 으로 상시 발화한다: plugin 별
    /// `S4a plugin_shutdown_one`(개별 ms + graceful/killed 사유) + 합계
    /// `S4 plugin_shutdown`(plugin 수). plugin 이 0개여도 S4 는 `plugins=0` 으로
    /// 반드시 발화한다 — "안 걸렸다" 와 "계측이 안 붙었다" 를 로그로 구분해야 한다.
    /// 마커 표는 본체 `docs/architecture/shutdown-sequence.md`.
    pub fn shutdown_all(&mut self) {
        self.begin_shutdown_all();
        while !self.poll_shutdown_all() {
            std::thread::sleep(CHILD_EXIT_POLL_INTERVAL);
        }
    }

    /// 전 plugin 에 shutdown 요청을 전송하고 **대기 없이** 즉시 반환한다.
    ///
    /// 요청을 먼저 전부 뿌리는 것이 대기 겹침의 전제다. 요청은 각 plugin 의
    /// `req_tx` 에 들어가므로, 앞서 dispatch 된 `surface.closed` 들보다 뒤에
    /// 놓인다는 채널 순서 계약(`src/app/shutdown_cascade.rs`)은 그대로 유지된다.
    ///
    /// 반환 후에는 [`Self::poll_shutdown_all`] 이 true 를 반환할 때까지 폴링해야
    /// 자식이 회수된다(폴링 없이 매니저가 drop 되면 남은 자식은 즉시 kill 된다).
    /// 이미 진행 중이면 no-op.
    pub fn begin_shutdown_all(&mut self) {
        if self.shutdown_batch.is_some() {
            return;
        }
        let deadline = Instant::now() + PLUGIN_SHUTDOWN_TIMEOUT;
        let pending: Vec<_> = self
            .processes
            .drain()
            .map(|(_, proc)| proc.begin_shutdown(deadline))
            .collect();
        self.shutdown_batch = Some(ShutdownBatch::new(pending));
    }

    /// 종료 대기 논블로킹 폴링. 남은 대상이 없으면 `true`(= 모든 자식 회수 완료).
    /// [`Self::begin_shutdown_all`] 없이 호출하면 기다릴 것이 없으므로 `true`.
    ///
    /// 완료된 plugin 마다 `S4a` 를 발화하고, 전부 끝난 라운드에 `S4` 를 발화한다.
    pub fn poll_shutdown_all(&mut self) -> bool {
        let Some(batch) = self.shutdown_batch.as_mut() else {
            return true;
        };
        for report in batch.poll() {
            tracing::info!(
                target: "tasty::shutdown",
                ms = report.elapsed.as_secs_f64() * 1000.0,
                plugin_id = report.plugin_id,
                reason = report.outcome.as_str(),
                "S4a plugin_shutdown_one (graceful deadline 2s)"
            );
        }
        if !batch.is_done() {
            return false;
        }
        let ms = batch.elapsed().as_secs_f64() * 1000.0;
        let plugins = batch.total();
        self.shutdown_batch = None;
        self.plugin_buffers.clear();
        tracing::info!(
            target: "tasty::shutdown",
            ms,
            plugins,
            "S4 plugin_shutdown (병렬 대기 합계)"
        );
        true
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
            if let Some(hh) = &self.hook_handler {
                hh.install_plugin_hook_handlers(plugin_id, &pkg.manifest.contributes.hook_handler);
            }
            if let Some(cs) = &self.completion_strategy {
                cs.install_plugin_completion_strategies(
                    completion_strategy_owner_id(&pkg),
                    &pkg.manifest.contributes.completion_strategy,
                );
            }

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
            proc.shutdown(PLUGIN_SHUTDOWN_TIMEOUT);
        }
        // ipc namespace 는 **해제하지 않는다.** disable 은 설치를 되돌리는 것이 아니라
        // 기동을 끄는 것이고, 소유는 설치 사실이다. 해제하면 그 plugin 의 메서드가
        // "그런 메서드 없다"(거짓)로 답한다 — 지금은 소유가 남아 forward 로 가고
        // `validate_namespace_call` 이 `-32002 plugin '…' is not running`(참)으로
        // 답한다([ADR-0173](../../../../docs/adr/0173-namespace-resolution-reads-the-manifest-not-the-process-table.md)).
        //
        // completion_strategy 의 owner id 는 install 시점과 동일 유도 규칙
        // (`completion_strategy_owner_id`)으로 계산해둔다 — install 은 ipc_namespace
        // 접두어를 owner 로 쓰므로 uninstall 도 같은 문자열로 지워야 매치된다(그냥
        // plugin_id 를 쓰면 등록은 되고 해제는 안 되는 stale 전략이 남는다).
        let cs_owner_id: Option<String> = self
            .packages
            .iter()
            .find(|p| p.manifest.id == plugin_id)
            .map(|pkg| completion_strategy_owner_id(pkg).to_string());
        // file_format / file_handler / hook_handler / completion_strategy registry 에서
        // plugin 의 contribute 제거.
        self.file_format.uninstall_plugin(plugin_id);
        self.file_handler.uninstall_plugin(plugin_id);
        if let Some(hh) = &self.hook_handler {
            hh.uninstall_plugin(plugin_id);
        }
        if let Some(cs) = &self.completion_strategy {
            cs.uninstall_plugin(cs_owner_id.as_deref().unwrap_or(plugin_id));
        }
        // `plugin.unloaded` / `plugin.disabled` 발화는 D.3.C.G.2.b cascade 가 처리
        // (App::plugin_disable 의 CoreEvent::PluginEnableToggled + PluginUnloaded
        // → cascade). was_running 분기는 App::plugin_disable 가 사전 캡처하므로 본
        // 메서드 안에서는 사용 안 함.
        let _ = was_running; // 의도적으로 무시 — 발화는 cascade 가 담당.
        self.event_bus.clear_plugin(plugin_id);
        self.cancel_pending_namespace_calls(plugin_id, "plugin disabled");
        self.plugin_buffers.remove(plugin_id);
        self.settings_pages.unregister_plugin(plugin_id);
        // registered_plugins gate 해제 — 이걸 안 지우면 재기동 후 새 프로세스의
        // hello 가 pump::classify_event 에서 "이미 등록됨"으로 오판돼
        // finalize_plugin_hello(→ hook_event_registry.register 등)가 재실행되지
        // 않는다. plugin_remove 도 내부적으로 이 disable() 을 거치므로 함께 커버된다.
        self.registered_plugins.remove(plugin_id);
        Ok(())
    }

    pub fn is_running(&self, plugin_id: &str) -> bool {
        self.processes.contains_key(plugin_id)
    }

    /// 호스트가 송신한 `surface.restore` 요청 중 plugin 응답을 아직 못 받은 것이
    /// 하나라도 있는지. 부팅 시 wait-for-plugin loop 가 round-trip 완료까지
    /// 기다리는 데 사용 — None 인 snapshot_cache 가 main loop 에 진입하지 못하게
    /// 함으로써 capture 시 kind="empty" fallback 으로 layout.json 이 오염되는
    /// race 를 차단.
    pub fn has_pending_surface_restores(&self) -> bool {
        self.pending_requests
            .values()
            .any(|k| matches!(k, super::PendingRequestKind::SurfaceRestore { .. }))
    }

    /// graceful swap 전용 — `config.disabled.ids` 를 건드리지 않고 process 만
    /// shutdown + 부속 registry 정리. `disable()` 의 sibling 인데 config persist
    /// 부작용이 없다 (verify-J-E §3.2: silent config corruption 회피).
    ///
    /// 호출 순서: `swap_shutdown_internal` → 외부에서 disk overwrite → `swap_respawn_internal`.
    /// upgrade_builtins 의 `--restart-running` flag 경로 외에서는 사용 금지.
    pub(crate) fn swap_shutdown_internal(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        if let Some(proc) = self.processes.remove(plugin_id) {
            proc.shutdown(PLUGIN_SHUTDOWN_TIMEOUT);
        }
        // ipc namespace 유지 — swap 중에 오는 호출은 "없는 메서드" 가 아니라
        // "지금 안 뜬 plugin" 이다(ADR-0173).
        self.event_bus.clear_plugin(plugin_id);
        self.cancel_pending_namespace_calls(plugin_id, "plugin swap restart");
        self.plugin_buffers.remove(plugin_id);
        self.settings_pages.unregister_plugin(plugin_id);
        // registered_plugins gate 해제 — disable() 과 동일한 이유(Task 14). 여기서
        // 안 지우면 swap_respawn_internal 이 띄운 새 프로세스의 hello 가 pump 의
        // "이미 등록됨" 게이트에 막혀 finalize_plugin_hello 재실행이 안 된다.
        self.registered_plugins.remove(plugin_id);
        Ok(())
    }

    /// graceful swap 전용 — `config.disabled.ids` 미수정 process spawn.
    /// `enable()` 의 sibling. 새 binary 로 재시작.
    pub(crate) fn swap_respawn_internal(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        self.auto_disabled.remove(plugin_id);
        let pkg = self
            .packages
            .iter()
            .find(|p| p.manifest.id == plugin_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("plugin '{plugin_id}' not in packages"))?;
        if self.processes.contains_key(plugin_id) {
            return Ok(());
        }
        self.ensure_listener();
        self.start_plugin_internal(&pkg);
        if !self.processes.contains_key(plugin_id) {
            anyhow::bail!("plugin '{plugin_id}' respawn failed (spawn error logged)");
        }
        Ok(())
    }

    /// 현재 `packages` + `config.is_disabled`를 기준으로 extension 상태를 재계산.
    /// 디스커버리/enable/disable/install/remove 후 매번 호출한다.
    pub fn log_path(&self, plugin_id: &str) -> PathBuf {
        self.log_dir.join(format!("{plugin_id}.log"))
    }

    /// H.d — auto-reload swap 한 건. `swap_shutdown_internal` → `swap_respawn_internal`
    /// 순으로 호출하고 성공 시 baseline 을 새 값으로 갱신해 다음 polling tick 의
    /// 무한 swap loop 를 차단한다.
    ///
    /// 실패 시:
    /// - `swap_shutdown_internal` 실패 — 옛 process 그대로. baseline 미갱신.
    /// - `swap_respawn_internal` 실패 — spawn_failures / auto_disabled 기존 로직이
    ///   3회 누적 시 plugin 을 차단. 옛 동작 (수동 disable/enable) 으로 graceful
    ///   degrade. 본 helper 는 baseline 만 갱신해 *부분 swap* (shutdown 만 성공)
    ///   상태에서도 같은 diff 로 재시도하지 않도록 한다.
    pub(super) fn auto_reload_one(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        tracing::info!("auto-reload: {plugin_id} swap start");
        self.swap_shutdown_internal(plugin_id)?;
        let respawn = self.swap_respawn_internal(plugin_id);
        // shutdown 이 성공한 시점에서 baseline 을 갱신해야 다음 polling tick 에서
        // 같은 mtime/version 으로 또 swap 시도하지 않는다 (respawn 결과와 무관).
        self.capture_plugin_baseline(plugin_id);
        respawn?;
        tracing::info!("auto-reload: {plugin_id} swap done");
        Ok(())
    }

    /// H.c — auto-reload 가 활성화된 상태에서 실행 중인 plugin 중 baseline 대비
    /// entry binary mtime 또는 manifest version 이 달라진 id 목록을 반환.
    ///
    /// 신호 조합: `binary mtime diff` OR `manifest version diff`.
    /// - metadata 읽기 실패 (binary 없음, 권한 등) 시 mtime 신호 skip — 무해.
    /// - flag off 면 즉시 빈 Vec — pump cost 0.
    /// - process 가 실행 중이지 않은 plugin 은 reload 대상 아님 — skip.
    pub(super) fn check_for_updates(&self) -> Vec<String> {
        if !self.auto_reload_enabled {
            return Vec::new();
        }
        let mut updated = Vec::new();
        for plugin_id in self.processes.keys() {
            let pkg = match self.packages.iter().find(|p| &p.manifest.id == plugin_id) {
                Some(p) => p,
                None => continue,
            };
            let bin = pkg.entry_command_path();
            let new_mtime = std::fs::metadata(&bin).ok().and_then(|m| m.modified().ok());
            let old_mtime = self.plugin_binary_mtimes.get(plugin_id).copied();
            let binary_changed = matches!((new_mtime, old_mtime), (Some(n), Some(o)) if n != o);

            let new_version = pkg.manifest.version.as_str();
            let version_changed = self
                .plugin_manifest_versions
                .get(plugin_id)
                .is_some_and(|old| old != new_version);

            if binary_changed || version_changed {
                updated.push(plugin_id.clone());
            }
        }
        updated
    }

    /// H.b — plugin 한 건의 baseline (entry binary mtime + manifest version) 캡처.
    /// spawn 성공 직후 + auto_reload swap 직후 호출하여 무한 reload loop 회피.
    /// metadata 실패 시 mtime 항목만 skip — version 은 항상 packages 캐시에서 갱신.
    pub(super) fn capture_plugin_baseline(&mut self, plugin_id: &str) {
        let pkg = match self.packages.iter().find(|p| p.manifest.id == plugin_id) {
            Some(p) => p,
            None => return,
        };
        let bin = pkg.entry_command_path();
        match std::fs::metadata(&bin).and_then(|m| m.modified()) {
            Ok(mtime) => {
                self.plugin_binary_mtimes
                    .insert(plugin_id.to_string(), mtime);
            }
            Err(e) => {
                tracing::debug!(
                    "plugin '{plugin_id}' baseline mtime skip ({}): {e}",
                    bin.display()
                );
                self.plugin_binary_mtimes.remove(plugin_id);
            }
        }
        self.plugin_manifest_versions
            .insert(plugin_id.to_string(), pkg.manifest.version.clone());
    }
}
