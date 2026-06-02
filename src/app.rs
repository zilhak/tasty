//! `App` — winit `ApplicationHandler` 의 본체. 다중 윈도우 + 모달 + 플러그인 매니저 +
//! parked AppState 보관. 메서드는 도메인별 서브모듈로 분산되어 있다.

#[cfg(feature = "gui")]
pub(crate) mod busy;
#[cfg(feature = "gui")]
pub(crate) mod clipboard_record;
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
pub(crate) mod ipc;
#[cfg(feature = "gui")]
pub(crate) mod modal;
#[cfg(feature = "gui")]
pub(crate) mod persistence;
#[cfg(feature = "gui")]
pub(crate) mod plugin_glue;
#[cfg(feature = "gui")]
pub(crate) mod request_owner;
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

pub(crate) struct App {
    /// Phase C — 도메인 본체. 마이그레이션 중에는 빈 골격, sub-step 마다 한 필드씩
    /// `CoreState` 에서 이쪽으로 이동한다.
    pub(crate) core: Core,
    /// Phase C — 외부 통신 표면. ipc_server, port_file 보유.
    pub(crate) hub: Hub,
    /// Phase C — GUI 어댑터. proxy, modal/focus 식별자, windows HashMap (예정) 보유.
    #[cfg(feature = "gui")]
    pub(crate) view: ViewRegistry,
    /// Parked AppStates: preserved when all windows are closed so PTY sessions survive.
    /// Moved into new windows when created, or used directly for IPC.
    #[cfg(feature = "gui")]
    pub(crate) parked_states: Vec<(state::AppState, crate::core::CoreState)>,
    // Shell setup mode (before terminal is created)
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_mode: bool,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_path: String,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_gpu: Option<GpuState>,
    #[cfg(feature = "gui")]
    pub(crate) shell_setup_window: Option<Arc<Window>>,
    /// System tray icon (Windows only). Must be kept alive for the tray to remain visible.
    #[cfg(all(windows, feature = "gui"))]
    pub(crate) tray_icon: Option<tray_icon::TrayIcon>,
    /// Tray menu item IDs for event matching (Windows only).
    #[cfg(all(windows, feature = "gui"))]
    pub(crate) tray_menu_ids: Option<crate::system_tray::TrayMenuIds>,
    /// Modal shake animation state.
    #[cfg(feature = "gui")]
    pub(crate) modal_shake: Option<ModalShake>,
    /// Whether input simulation IPC is enabled (debug builds only).
    #[cfg(debug_assertions)]
    pub(crate) input_simulation_enabled: bool,
    /// Plugin host manager. None until the first AppState is created
    /// (which provides the WakerFactory).
    pub(crate) plugin_manager: Option<plugin::PluginManager>,
    /// Sessionwide engine state — workspaces, settings, hooks, registries.
    /// None until the first MainView lifecycle initializes it; Some after.
    pub(crate) core_state: Option<crate::core::CoreState>,
    /// 사용자 init.lua 기반 Lua hook 엔진. 부팅 시 1회 생성, `~/.tasty/init.lua` 가
    /// 있으면 로드. observe-only — 호스트 동작에는 영향 없음. 초기화 실패 시 None.
    pub(crate) lua_engine: Option<tasty_lua::LuaEngine>,
    /// 현재 열려 있는 `PresetWindow` 의 winit window id. modeless editor 윈도우는
    /// 엔진 전역 단일 인스턴스 — 같은 명령이 다시 들어오면 새 윈도우를 만들지 않고
    /// 이 id 의 윈도우로 포커스만 이동한다.
    #[cfg(feature = "gui")]
    pub(crate) preset_window_id: Option<WindowId>,
}

/// State for the modal window shake animation.
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
        Ok(Self {
            core: crate::boot::wiring::build_production_core(proxy.clone(), memory)?,
            hub: Hub::new(port_file),
            view: ViewRegistry::new(proxy.clone()),
            parked_states: Vec::new(),
            shell_setup_mode: false,
            shell_setup_path: String::new(),
            shell_setup_gpu: None,
            shell_setup_window: None,
            #[cfg(windows)]
            tray_icon: None,
            #[cfg(windows)]
            tray_menu_ids: None,
            modal_shake: None,
            #[cfg(debug_assertions)]
            input_simulation_enabled,
            plugin_manager: None,
            core_state: None,
            lua_engine: crate::hooks::lua::init_engine(),
            preset_window_id: None,
        })
    }

    /// Headless 부트 — winit/wgpu/egui 0. mpsc 기반 waker.
    #[cfg(not(feature = "gui"))]
    pub(crate) fn new_headless(
        terminal_waker: std::sync::Arc<dyn crate::ports::pty::TerminalWaker>,
        port_file: Option<String>,
        memory: Option<std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            core: crate::boot::wiring::build_production_core_headless(terminal_waker, memory)?,
            hub: Hub::new(port_file),
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
            plugin_manager: None,
            core_state: None,
            lua_engine: crate::hooks::lua::init_engine(),
        })
    }

    /// CoreState 접근자. 부팅 시 `App.core_state` 에 들어 있다가 첫 MainView
    /// 등록 시 그쪽으로 이동한다. 이 헬퍼는 두 위치 중 살아있는 쪽을 찾아 반환한다.
    /// 어디에도 없으면 panic — 호출 경로가 invariant 를 깬 것.
    pub(crate) fn core_state(&self) -> &crate::core::CoreState {
        if let Some(e) = self.core_state.as_ref() {
            return e;
        }
        #[cfg(feature = "gui")]
        for w in self.view.windows.values() {
            if let Some(main) = w.as_main() {
                return &main.core_state;
            }
        }
        panic!("App.core_state accessed before initialization");
    }

    pub(crate) fn core_state_mut(&mut self) -> &mut crate::core::CoreState {
        if self.core_state.is_some() {
            return self.core_state.as_mut().unwrap();
        }
        #[cfg(feature = "gui")]
        for w in self.view.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                return &mut main.core_state;
            }
        }
        panic!("App.core_state accessed before initialization");
    }
}
