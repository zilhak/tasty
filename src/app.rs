//! `App`은 winit 이벤트 처리, 창·모달·플러그인, 창 없이 보존하는 세션 상태를 관리한다.

/// GUI와 헤드리스에서 공통으로 이벤트 큐를 비운다.
pub(crate) mod agent_events;
#[cfg(feature = "gui")]
pub(crate) mod attach_client;
#[cfg(feature = "gui")]
pub(crate) mod attach_poll;
pub(crate) mod creation_intent;
#[cfg(feature = "gui")]
pub(crate) mod auto_attach;
#[cfg(feature = "gui")]
pub(crate) mod boot_machine;
#[cfg(feature = "gui")]
pub(crate) mod busy;
#[cfg(all(debug_assertions, feature = "gui"))]
pub(crate) mod debug_info;
#[cfg(feature = "gui")]
pub(crate) mod dispatch;
#[cfg(feature = "gui")]
pub(crate) mod dispatch_domain;
#[cfg(not(feature = "gui"))]
#[path = "app/dispatch_domain_stubs.rs"]
pub(crate) mod dispatch_domain;
#[cfg(feature = "gui")]
pub(crate) mod engine_registry;
pub(crate) mod event;
#[cfg(feature = "gui")]
pub(crate) mod event_handler;
#[cfg(feature = "gui")]
mod file_dispatch;
#[cfg(feature = "gui")]
pub(crate) mod global_hooks;
#[cfg(feature = "gui")]
pub(crate) mod idle_hooks;
#[cfg(feature = "gui")]
pub(crate) mod image_upload;
#[cfg(feature = "gui")]
pub(crate) mod ipc;
pub(crate) mod ipc_round;
pub(crate) mod journal;
pub(crate) mod services;
pub(crate) mod state;
pub(crate) mod command;
#[cfg(feature = "gui")]
pub(crate) mod modal;
#[cfg(feature = "gui")]
pub(crate) mod persistence;
#[cfg(feature = "gui")]
pub(crate) mod plugin_glue;
pub(crate) mod process_exit;
pub(crate) mod attach_activation;
pub(crate) mod publication_input;
#[cfg(feature = "gui")]
pub(crate) mod request_owner;
#[cfg(feature = "gui")]
pub(crate) mod screenshot_capture;
#[cfg(feature = "gui")]
pub(crate) mod shutdown_cascade;
#[cfg(feature = "gui")]
pub(crate) mod shutdown_machine;
#[cfg(feature = "gui")]
pub(crate) mod shutdown_trace;
#[cfg(feature = "gui")]
pub(crate) mod sweeps;
pub(crate) mod timer_report;
pub(crate) mod timers;
#[cfg(feature = "gui")]
pub(crate) mod webview_keys;
#[cfg(feature = "gui")]
pub(crate) mod window_access;
#[cfg(feature = "gui")]
pub(crate) mod window_lifecycle;

#[cfg(feature = "gui")]
use std::sync::Arc;

#[cfg(feature = "gui")]
use winit::event_loop::EventLoopProxy;
#[cfg(feature = "gui")]
use winit::window::{Window, WindowId};

use crate::app::services::AppServices;
#[cfg(feature = "gui")]
use crate::gpu::GpuState;
use crate::hub::Hub;
#[cfg(not(feature = "gui"))]
use crate::plugin;
#[cfg(feature = "gui")]
use crate::view::ViewRegistry;
#[cfg(feature = "gui")]
use crate::{AppEvent, plugin};

/// GPU 어댑터를 찾지 못한 오류. 호출자가 downcast하여 사용자에게 안내한다.
#[cfg(feature = "gui")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NoGpuAdapter;

#[cfg(feature = "gui")]
impl std::fmt::Display for NoGpuAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no compatible GPU adapter found (hardware or software fallback)"
        )
    }
}

#[cfg(feature = "gui")]
impl std::error::Error for NoGpuAdapter {}

pub(crate) struct App {
    pub(crate) journal: journal::JournalApplication,
    #[cfg(feature="gui")]
    pub(crate) settings_edit_owner:Option<settings_edit::SettingsEditOwner>,
    #[cfg(feature="gui")]
    pub(crate) port_scans:port_scans::PortScans,
    pub(crate) pending_server_attaches:Vec<attach_activation::PendingAttach>,
    pub(crate) publication_inputs: publication_input::PublicationInputs,
    pub(crate) services: AppServices,
    pub(crate) state:state::AppState,
    pub(crate) hub: Hub,
    /// IPC 연결 스레드가 수신자를 등록하고 메인 루프가 출력을 전송한다.
    pub(crate) stream_hub: tasty_ipc::stream_hub::StreamHub,
    /// Sender cloned into each stream connection so its read thread can route
    /// inbound frames to the main loop.
    pub(crate) stream_inbound_tx: std::sync::mpsc::Sender<tasty_ipc::stream_hub::StreamInbound>,
    /// Receiver drained by the main loop on `AppEvent::StreamReady`.
    pub(crate) stream_inbound_rx: std::sync::mpsc::Receiver<tasty_ipc::stream_hub::StreamInbound>,
    #[cfg(feature = "gui")]
    pub(crate) view: ViewRegistry,
    /// 모든 engine과 창·parked·임시 관계. 창을 모두 닫아도 engine과 PTY는 여기 남는다.
    /// `view` 바로 뒤에 두어 종료 때 모든 창 View가 먼저, 그 뒤 engine이 drop된다.
    #[cfg(feature = "gui")]
    pub(crate) engines: engine_registry::EngineRegistry,
    /// 첫 창의 부팅 중에만 보유한다. 완료 시 상태를 MainView로 옮긴다.
    #[cfg(feature = "gui")]
    pub(crate) boot: Option<boot_machine::BootResources>,
    #[cfg(feature = "gui")]
    pub(crate) pending_window: Option<window_lifecycle::PendingWindow>,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_gpu: Option<GpuState>,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_window: Option<Arc<Window>>,
    #[cfg(feature = "gui")]
    pub(crate) boot_error_gpu: Option<GpuState>,
    #[cfg(feature = "gui")]
    pub(crate) boot_error_window: Option<Arc<Window>>,
    /// System tray / status item. Must be kept alive for the tray to remain visible.
    /// `None` when the platform tray is unavailable (graceful degradation, ADR-0016).
    #[cfg(all(
        any(windows, target_os = "macos", target_os = "linux"),
        feature = "gui"
    ))]
    pub(crate) tray_icon: Option<tray_icon::TrayIcon>,
    #[cfg(all(
        any(windows, target_os = "macos", target_os = "linux"),
        feature = "gui"
    ))]
    pub(crate) tray_menu_ids: Option<crate::system_tray::TrayMenuIds>,
    #[cfg(feature = "gui")]
    pub(crate) modal_shake: Option<ModalShake>,
    pub(crate) plugin_manager: Option<plugin::PluginManager>,
    /// VM은 전용 워커 스레드가 소유한다. 초기화 실패 시 None.
    pub(crate) lua_engine: Option<tasty_lua::LuaEngine>,
    /// 자동실행 스크립트가 일으킨 이벤트로 같은 스크립트를 다시 실행하지 않게 한다.
    /// `about_to_wait` 시작 시 [`checkpoint`](crate::hooks::autofire::AutofireGuard::checkpoint)로 회차를 갱신한다.
    pub(crate) lua_autofire: crate::hooks::autofire::AutofireGuard,
    /// GUI와 헤드리스의 주기 작업을 예약한다. docs/dev-guide/timer-hub.md 참조.
    pub(crate) timers: tasty_timer::TimerHub<timers::Tick>,
    /// 창이 없거나 최소화되어도 send_event로 타이머 처리를 깨운다.
    #[cfg(feature = "gui")]
    pub(crate) timer_waker: tasty_timer::TimerWakerHandle,
    /// IPC 회차 사이에 대기 시간을 두어 타이머·사용자 이벤트 처리가 계속되게 한다.
    #[cfg(feature = "gui")]
    pub(crate) ipc_pacer: crate::app::ipc::IpcPacer,
    /// 첫 Focused 이벤트 뒤 X11 초기 포커스 힌트를 지울 에이전트 창.
    /// 그 전에 닫힌 창은 닫기 경로에서 제거한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_focus_hint_clear: std::collections::HashSet<WindowId>,
    #[cfg(feature="gui")]
    pub(crate) remote:tasty_remote::outbound::Remote,
    /// 스크린샷→클립보드 캡처 워커 스레드 → 메인 루프 결과 채널.
    #[cfg(feature = "gui")]
    pub(crate) screenshot_capture_tx:
        std::sync::mpsc::Sender<screenshot_capture::ScreenshotCaptureOutcome>,
    #[cfg(feature = "gui")]
    pub(crate) screenshot_capture_rx:
        std::sync::mpsc::Receiver<screenshot_capture::ScreenshotCaptureOutcome>,
    /// mirror 이미지 paste 업로드 워커 스레드 → 메인 루프 결과 채널.
    #[cfg(feature = "gui")]
    pub(crate) image_upload_tx: std::sync::mpsc::Sender<image_upload::ImageUploadOutcome>,
    #[cfg(feature = "gui")]
    pub(crate) image_upload_rx: std::sync::mpsc::Receiver<image_upload::ImageUploadOutcome>,
    /// 업로드 워커의 진행 정보를 메인 루프로 전달한다.
    #[cfg(feature = "gui")]
    pub(crate) transfer_progress_tx: std::sync::mpsc::Sender<image_upload::TransferProgressMsg>,
    #[cfg(feature = "gui")]
    pub(crate) transfer_progress_rx: std::sync::mpsc::Receiver<image_upload::TransferProgressMsg>,
    /// 창마다 Instance를 만들지 않도록 공유한다. 모든 surface보다 오래 유지한다.
    #[cfg(feature = "gui")]
    pub(crate) gpu_instance: Arc<wgpu::Instance>,
    /// 첫 창의 surface로 선택한 어댑터를 다른 창에서도 재사용한다.
    #[cfg(feature = "gui")]
    pub(crate) gpu_adapter: Option<Arc<wgpu::Adapter>>,
}

#[cfg(feature = "gui")]
pub(crate) struct ModalShake {
    pub(crate) start: std::time::Instant,
    /// Original window position before shake began.
    pub(crate) origin: winit::dpi::PhysicalPosition<i32>,
}

impl App {
    #[cfg(feature = "gui")]
    pub(crate) fn new(
        proxy: EventLoopProxy<AppEvent>,
        port_file: Option<String>,
        memory: Option<std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>>,
        #[cfg(debug_assertions)] input_simulation_enabled: bool,
    ) -> anyhow::Result<Self> {
        let (stream_inbound_tx, stream_inbound_rx) = std::sync::mpsc::channel();
        let (screenshot_capture_tx, screenshot_capture_rx) = std::sync::mpsc::channel();
        let (image_upload_tx, image_upload_rx) = std::sync::mpsc::channel();
        let (transfer_progress_tx, transfer_progress_rx) = std::sync::mpsc::channel();
        let mut timers = tasty_timer::TimerHub::new();
        timers::register_steady_state(&mut timers, std::time::Instant::now());
        let timer_waker = tasty_timer::spawn_timer_waker({
            let proxy = proxy.clone();
            move || proxy.send_event(AppEvent::TimerTick).is_ok()
        });
        let journal = journal::JournalApplication::new({
            let proxy = proxy.clone();
            Arc::new(move || {
                if proxy.send_event(AppEvent::JournalReady).is_err() {
                    tracing::trace!("journal wake after GUI shutdown");
                }
            })
        })?;
        Ok(Self {
            journal,pending_server_attaches:Vec::new(),settings_edit_owner:None,port_scans:Default::default(),
            publication_inputs: Default::default(),
            services: crate::boot::wiring::build_production_core(memory)?,
            state:state::AppState {#[cfg(debug_assertions)] input_simulation_enabled,..Default::default()},
            hub: Hub::new(port_file),
            stream_hub: tasty_ipc::stream_hub::StreamHub::new(),
            stream_inbound_tx,
            stream_inbound_rx,
            view: ViewRegistry::new(proxy.clone()),
            engines: engine_registry::EngineRegistry::default(),
            boot: None,
            pending_window: None,
            shell_setup_gpu: None,
            shell_setup_window: None,
            boot_error_gpu: None,
            boot_error_window: None,
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            tray_icon: None,
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            tray_menu_ids: None,
            modal_shake: None,
            plugin_manager: None,
            lua_engine: crate::hooks::lua::init_engine(),
            lua_autofire: crate::hooks::autofire::AutofireGuard::new(),
            timers,
            timer_waker,
            ipc_pacer: crate::app::ipc::IpcPacer::new({
                let proxy = proxy.clone();
                Box::new(move || {
                    crate::shortcuts::send_app_event(&proxy, AppEvent::IpcReady);
                })
            }),
            pending_focus_hint_clear: std::collections::HashSet::new(),
            remote:tasty_remote::outbound::Remote::new(),
            screenshot_capture_tx,
            screenshot_capture_rx,
            image_upload_tx,
            image_upload_rx,
            transfer_progress_tx,
            transfer_progress_rx,
            gpu_instance: Arc::new(wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..Default::default()
            })),
            gpu_adapter: None,
        })
    }

    /// GUI 자원 없이 mpsc waker로 시작한다.
    #[cfg(not(feature = "gui"))]
    pub(crate) fn new_headless(
        journal_wake: std::sync::Arc<dyn Fn() + Send + Sync>,
        port_file: Option<String>,
        memory: Option<std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>>,
    ) -> anyhow::Result<Self> {
        let (stream_inbound_tx, stream_inbound_rx) = std::sync::mpsc::channel();
        let mut timers = tasty_timer::TimerHub::new();
        timers::register_steady_state(&mut timers, std::time::Instant::now());
        Ok(Self {
            journal: journal::JournalApplication::new(journal_wake)?,pending_server_attaches:Vec::new(),
            publication_inputs: Default::default(),
            services: crate::boot::wiring::build_production_core_headless(memory)?,
            state:state::AppState::default(),
            hub: Hub::new(port_file),
            stream_hub: tasty_ipc::stream_hub::StreamHub::new(),
            stream_inbound_tx,
            stream_inbound_rx,
            plugin_manager: None,
            lua_engine: crate::hooks::lua::init_engine(),
            lua_autofire: crate::hooks::autofire::AutofireGuard::new(),
            timers,
        })
    }

    /// 자동실행은 CoreState 초기화 전에도 호출될 수 있어 그때는 빈 레지스트리를 반환한다.
    #[cfg(feature = "gui")]
    pub(crate) fn autofire_scripts(&self) -> tasty_settings::ScriptRegistry {
        let engines = self.engines();
        engines
            .primary()
            .or_else(|| engines.parked().next())
            .map(|e| e.runtime.settings.scripts.clone())
            .unwrap_or_default()
    }

    /// 창별 GPU 상태를 만들되 instance와 첫 창에서 선택한 adapter는 공유한다.
    #[cfg(feature = "gui")]
    pub(crate) fn create_gpu_state(
        &mut self,
        window: Arc<Window>,
        appearance: &crate::settings::AppearanceSettings,
    ) -> anyhow::Result<GpuState> {
        let instance = Arc::clone(&self.gpu_instance);
        // 첫 창은 CoreState보다 먼저 만들어질 수 있어 없으면 기본 휠 거리를 사용한다.
        let wheel_line_scroll = self
            .engines()
            .primary()
            .map(|cs| cs.settings.general.wheel_line_scroll)
            .unwrap_or(tasty_settings::DEFAULT_WHEEL_LINE_SCROLL);
        // 모달의 CoreState가 아직 없을 수 있으므로 배율은 이 창의 appearance를 사용한다.
        let theme_runtime = tasty_themes::ThemeRuntime {
            ui_zoom: appearance.ui_scale_factor(),
            ..self
                .engines()
                .primary()
                .map(|cs| cs.settings.theme_runtime())
                .unwrap_or_default()
        };
        let proxy = self.view.proxy.clone();
        pollster::block_on(async move {
            if self.gpu_adapter.is_none() {
                // 어댑터 선택용 surface는 실제 surface를 만들기 전에 해제한다.
                let adapter = {
                    let probe = instance.create_surface(window.clone())?;
                    let opts = wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::default(),
                        compatible_surface: Some(&probe),
                        force_fallback_adapter: false,
                    };
                    // 하드웨어 선택에 실패하면 소프트웨어 어댑터도 시도한다.
                    match instance.request_adapter(&opts).await {
                        Some(a) => a,
                        None => instance
                            .request_adapter(&wgpu::RequestAdapterOptions {
                                force_fallback_adapter: true,
                                ..opts
                            })
                            .await
                            .ok_or_else(|| anyhow::Error::new(NoGpuAdapter))?,
                    }
                };
                self.gpu_adapter = Some(Arc::new(adapter));
            }
            let adapter = Arc::clone(
                self.gpu_adapter
                    .as_ref()
                    .expect("gpu_adapter set above when None"),
            );
            GpuState::new_shared(
                &instance,
                &adapter,
                window,
                appearance,
                theme_runtime,
                wheel_line_scroll,
                proxy,
            )
            .await
        })
    }
}

pub(crate) mod task_completion;

pub(crate) mod engine_action;

#[cfg(feature="gui")]
mod view_frame;

#[cfg(feature="gui")]
mod view_mesh;

#[cfg(feature="gui")]
mod explorer_action;

pub(crate) mod telemetry;

#[cfg(feature="gui")]
pub(crate) mod settings_edit;

#[cfg(feature="gui")]
pub(crate) mod remote_browser;

#[cfg(feature="gui")]
pub(crate) mod plugin_display;

#[cfg(feature="gui")]
pub(crate) mod settings_files;
#[cfg(feature="gui")]
mod preset_editor;

mod preset_capture;

#[cfg(feature="gui")]
mod port_scans;
