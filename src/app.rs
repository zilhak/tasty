//! `App`은 winit 이벤트 처리, 창·모달·플러그인, 창 없이 보존하는 세션 상태를 관리한다.

/// GUI와 헤드리스에서 공통으로 이벤트 큐를 비운다.
pub(crate) mod agent_events;
#[cfg(feature = "gui")]
pub(crate) mod attach_client;
#[cfg(feature = "gui")]
pub(crate) mod attach_poll;
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
#[cfg(feature = "gui")]
pub(crate) mod modal;
#[cfg(feature = "gui")]
pub(crate) mod persistence;
#[cfg(feature = "gui")]
pub(crate) mod plugin_glue;
pub(crate) mod process_exit;
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

use crate::core::Core;
#[cfg(feature = "gui")]
use crate::gpu::GpuState;
use crate::hub::Hub;
#[cfg(not(feature = "gui"))]
use crate::plugin;
#[cfg(feature = "gui")]
use crate::view::ViewRegistry;
#[cfg(feature = "gui")]
use crate::{AppEvent, plugin, state};

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

#[cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "some fields are read only by the gui event loop; headless has no reader"
    )
)]
pub(crate) struct App {
    pub(crate) core: Core,
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
    /// Parked AppStates: preserved when all windows are closed so PTY sessions survive.
    /// Moved into new windows when created, or used directly for IPC.
    #[cfg(feature = "gui")]
    pub(crate) parked_states: Vec<(state::AppState, crate::core::CoreState)>,
    /// 첫 창의 부팅 중에만 보유한다. 완료 시 상태를 MainView로 옮긴다.
    #[cfg(feature = "gui")]
    pub(crate) boot: Option<boot_machine::BootState>,
    /// 종료를 시작한 뒤 이벤트 루프를 나갈 때까지 보유한다.
    #[cfg(feature = "gui")]
    pub(crate) shutdown: Option<shutdown_machine::ShutdownState>,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_mode: bool,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_path: String,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_gpu: Option<GpuState>,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_window: Option<Arc<Window>>,
    // 엔진 생성 실패 시 창과 GPU가 남아 있으면 종료 버튼이 있는 오류 화면을 유지한다.
    // 창이나 GPU도 없으면 이 경로로 화면을 표시할 수 없다.
    #[cfg(feature = "gui")]
    pub(crate) boot_error_mode: bool,
    #[cfg(feature = "gui")]
    pub(crate) boot_error_gpu: Option<GpuState>,
    #[cfg(feature = "gui")]
    pub(crate) boot_error_window: Option<Arc<Window>>,
    /// `drive_boot_frame`이 오류 화면으로 전환할 때 표시할 진단.
    #[cfg(feature = "gui")]
    pub(crate) boot_error_info: Option<crate::gpu::BootErrorInfo>,
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
    #[cfg(debug_assertions)]
    pub(crate) input_simulation_enabled: bool,
    pub(crate) plugin_manager: Option<plugin::PluginManager>,
    /// 헤드리스의 전체 플러그인 설치·기동을 수행했는지 구별한다.
    /// 메타데이터 조회만으로도 매니저는 만들어지므로 Some 여부로 대신할 수 없다.
    #[cfg(not(feature = "gui"))]
    pub(crate) plugin_started: bool,
    /// 창에 배정하기 전의 CoreState. 창 생성 시 MainView로 옮긴다.
    pub(crate) core_state: Option<crate::core::CoreState>,
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
    /// PresetView는 하나만 열며, 다시 열기를 요청하면 기존 창에 포커스를 준다.
    #[cfg(feature = "gui")]
    pub(crate) preset_view_id: Option<WindowId>,
    /// 첫 Focused 이벤트 뒤 X11 초기 포커스 힌트를 지울 에이전트 창.
    /// 그 전에 닫힌 창은 닫기 경로에서 제거한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_focus_hint_clear: std::collections::HashSet<WindowId>,
    /// Plugins의 Configure에서 설정을 열 때 Plugin 탭을 한 번 선택한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_settings_plugin_tab: bool,
    /// 파일 핸들러 등록 안내에서 설정을 열 때 FileHandler 탭을 한 번 선택한다.
    #[cfg(feature = "gui")]
    pub(crate) pending_settings_file_handler_tab: bool,
    /// 부팅 권한 안내에서 설정 창을 열 때 일반 > 권한 탭으로 바로 들어가게 하는
    /// 일회성 표시. `open_settings_modal`이 읽고 지운다.
    #[cfg(feature = "gui")]
    pub(crate) pending_settings_macos_permissions_tab: bool,
    /// debug.settings.open이 지정한 초기 탭. 다음 설정 창 열기에서 소비한다.
    #[cfg(all(feature = "gui", debug_assertions))]
    pub(crate) pending_settings_tab: Option<String>,
    /// debug.settings.open이 지정한 초기 하위 탭. 다음 설정 창 열기에서 소비한다.
    #[cfg(all(feature = "gui", debug_assertions))]
    pub(crate) pending_settings_subtab: Option<String>,
    /// 원격 워크스페이스의 mirror 세션. 연결 스레드와 remote↔local ID 매핑을 보유한다.
    #[cfg(feature = "gui")]
    pub(crate) attach_client_sessions: Vec<attach_client::AttachClientSession>,
    /// 중복 자동 attach를 막기 위한 진행 중·완료된 anchor ID. 세션 정리 시 제거한다.
    #[cfg(feature = "gui")]
    pub(crate) auto_attach_active: std::collections::HashSet<u32>,
    /// 워크스페이스를 다시 활성화했는지 확인할 직전 활성 ID.
    #[cfg(feature = "gui")]
    pub(crate) auto_attach_last_active_ws: Option<u32>,
    /// 연결 해제 뒤 워크스페이스 재활성화를 기다리는 anchor.
    /// 새로 매핑한 활성 워크스페이스는 즉시 attach해야 하므로 별도로 구분한다.
    #[cfg(feature = "gui")]
    pub(crate) auto_attach_pending_reactivation: std::collections::HashSet<u32>,
    /// 자동 attach 워커의 SSH 터널·포트 결과를 메인 루프로 전달한다.
    #[cfg(feature = "gui")]
    pub(crate) auto_attach_tx: std::sync::mpsc::Sender<auto_attach::AutoAttachOutcome>,
    #[cfg(feature = "gui")]
    pub(crate) auto_attach_rx: std::sync::mpsc::Receiver<auto_attach::AutoAttachOutcome>,
    /// 끊긴 anchor별 재연결 시각·백오프·시도 횟수.
    /// 성공하거나 사용자가 mirror를 닫으면 제거한다.
    /// docs/dev-guide/attach-behavior.md#gui-자동-재연결-스코프 참조.
    #[cfg(feature = "gui")]
    pub(crate) auto_attach_reconnect: std::collections::HashMap<u32, auto_attach::ReconnectSlot>,
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
        let (auto_attach_tx, auto_attach_rx) = std::sync::mpsc::channel();
        let (screenshot_capture_tx, screenshot_capture_rx) = std::sync::mpsc::channel();
        let (image_upload_tx, image_upload_rx) = std::sync::mpsc::channel();
        let (transfer_progress_tx, transfer_progress_rx) = std::sync::mpsc::channel();
        let mut timers = tasty_timer::TimerHub::new();
        timers::register_steady_state(&mut timers, std::time::Instant::now());
        let timer_waker = tasty_timer::spawn_timer_waker({
            let proxy = proxy.clone();
            move || proxy.send_event(AppEvent::TimerTick).is_ok()
        });
        Ok(Self {
            core: crate::boot::wiring::build_production_core(memory)?,
            hub: Hub::new(port_file),
            stream_hub: tasty_ipc::stream_hub::StreamHub::new(),
            stream_inbound_tx,
            stream_inbound_rx,
            view: ViewRegistry::new(proxy.clone()),
            parked_states: Vec::new(),
            boot: None,
            shutdown: None,
            shell_setup_mode: false,
            shell_setup_path: String::new(),
            shell_setup_gpu: None,
            shell_setup_window: None,
            boot_error_mode: false,
            boot_error_gpu: None,
            boot_error_window: None,
            boot_error_info: None,
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            tray_icon: None,
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            tray_menu_ids: None,
            modal_shake: None,
            #[cfg(debug_assertions)]
            input_simulation_enabled,
            plugin_manager: None,
            #[cfg(not(feature = "gui"))]
            plugin_started: false,
            core_state: None,
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
            preset_view_id: None,
            pending_focus_hint_clear: std::collections::HashSet::new(),
            pending_settings_plugin_tab: false,
            pending_settings_file_handler_tab: false,
            pending_settings_macos_permissions_tab: false,
            #[cfg(debug_assertions)]
            pending_settings_tab: None,
            #[cfg(debug_assertions)]
            pending_settings_subtab: None,
            attach_client_sessions: Vec::new(),
            auto_attach_active: std::collections::HashSet::new(),
            auto_attach_last_active_ws: None,
            auto_attach_pending_reactivation: std::collections::HashSet::new(),
            auto_attach_tx,
            auto_attach_rx,
            auto_attach_reconnect: std::collections::HashMap::new(),
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
        port_file: Option<String>,
        memory: Option<std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>>,
    ) -> anyhow::Result<Self> {
        let (stream_inbound_tx, stream_inbound_rx) = std::sync::mpsc::channel();
        let mut timers = tasty_timer::TimerHub::new();
        timers::register_steady_state(&mut timers, std::time::Instant::now());
        Ok(Self {
            core: crate::boot::wiring::build_production_core_headless(memory)?,
            hub: Hub::new(port_file),
            stream_hub: tasty_ipc::stream_hub::StreamHub::new(),
            stream_inbound_tx,
            stream_inbound_rx,
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
            plugin_manager: None,
            #[cfg(not(feature = "gui"))]
            plugin_started: false,
            core_state: None,
            lua_engine: crate::hooks::lua::init_engine(),
            lua_autofire: crate::hooks::autofire::AutofireGuard::new(),
            timers,
        })
    }

    /// App 또는 MainView의 CoreState를 반환한다. 아직 초기화되지 않았으면 panic한다.
    #[cfg(feature = "gui")]
    pub(crate) fn core_state(&self) -> &crate::core::CoreState {
        if let Some(e) = self.core_state.as_ref() {
            return e;
        }
        #[cfg(feature = "gui")]
        for w in self.view.views.values() {
            if let Some(main) = w.as_main() {
                return &main.core_state;
            }
        }
        panic!("App.core_state accessed before initialization");
    }

    /// 자동실행은 CoreState 초기화 전에도 호출될 수 있어 그때는 빈 레지스트리를 반환한다.
    #[cfg(feature = "gui")]
    pub(crate) fn autofire_scripts(&self) -> tasty_settings::ScriptRegistry {
        if let Some(cs) = self.core_state.as_ref() {
            return cs.settings.scripts.clone();
        }
        #[cfg(feature = "gui")]
        {
            for w in self.view.views.values() {
                if let Some(main) = w.as_main() {
                    return main.core_state.settings.scripts.clone();
                }
            }
            if let Some((_, e)) = self.parked_states.first() {
                return e.settings.scripts.clone();
            }
        }
        tasty_settings::ScriptRegistry::default()
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
            .core_state
            .as_ref()
            .map(|cs| cs.settings.general.wheel_line_scroll)
            .or_else(|| {
                self.view.views.values().find_map(|w| {
                    w.as_main()
                        .map(|m| m.core_state.settings.general.wheel_line_scroll)
                })
            })
            .unwrap_or(tasty_settings::DEFAULT_WHEEL_LINE_SCROLL);
        // 모달의 CoreState가 아직 없을 수 있으므로 배율은 이 창의 appearance를 사용한다.
        let theme_runtime = tasty_themes::ThemeRuntime {
            ui_zoom: appearance.ui_scale_factor(),
            ..self
                .core_state
                .as_ref()
                .map(|cs| cs.settings.theme_runtime())
                .or_else(|| {
                    self.view
                        .views
                        .values()
                        .find_map(|w| w.as_main().map(|m| m.core_state.settings.theme_runtime()))
                })
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

    #[cfg(feature = "gui")]
    pub(crate) fn core_state_mut(&mut self) -> &mut crate::core::CoreState {
        if let Some(cs) = self.core_state.as_mut() {
            return cs;
        }
        #[cfg(feature = "gui")]
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                return &mut main.core_state;
            }
        }
        panic!("App.core_state accessed before initialization");
    }
}
