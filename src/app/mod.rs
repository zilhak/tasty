//! `App` — winit `ApplicationHandler` 의 본체. 다중 윈도우 + 모달 + 플러그인 매니저 +
//! parked AppState 보관. 메서드는 도메인별 서브모듈로 분산되어 있다.

pub(crate) mod busy;
pub(crate) mod clipboard_record;
pub(crate) mod dispatch;
pub(crate) mod ipc;
pub(crate) mod modal;
pub(crate) mod persistence;
pub(crate) mod plugin_glue;
pub(crate) mod window_access;
pub(crate) mod window_lifecycle;

use std::sync::Arc;

use winit::event_loop::EventLoopProxy;
use winit::window::{Window, WindowId};

use crate::gpu::GpuState;
use crate::{AppEvent, engine, plugin, state, window};

pub(crate) struct App {
    pub(crate) engine: engine::Engine,
    /// 모든 윈도우(모달 포함). `engine.active_modal_id`로 현재 활성 모달을 식별한다.
    /// 모달도 여기에 들어가며, 모달은 엔진 전역에 최대 1개라는 불변식을 유지한다.
    pub(crate) windows: std::collections::HashMap<WindowId, Box<dyn window::Window>>,
    /// Parked AppStates: preserved when all windows are closed so PTY sessions survive.
    /// Moved into new windows when created, or used directly for IPC.
    pub(crate) parked_states: Vec<state::AppState>,
    // Shell setup mode (before terminal is created)
    pub(crate) shell_setup_mode: bool,
    pub(crate) shell_setup_path: String,
    pub(crate) shell_setup_gpu: Option<GpuState>,
    pub(crate) shell_setup_window: Option<Arc<Window>>,
    /// System tray icon (Windows only). Must be kept alive for the tray to remain visible.
    #[cfg(windows)]
    pub(crate) tray_icon: Option<tray_icon::TrayIcon>,
    /// Tray menu item IDs for event matching (Windows only).
    #[cfg(windows)]
    pub(crate) tray_menu_ids: Option<crate::system_tray::TrayMenuIds>,
    /// Modal shake animation state.
    pub(crate) modal_shake: Option<ModalShake>,
    /// Whether input simulation IPC is enabled (debug builds only).
    #[cfg(debug_assertions)]
    pub(crate) input_simulation_enabled: bool,
    /// Plugin host manager. None until the first AppState is created
    /// (which provides the WakerFactory).
    pub(crate) plugin_manager: Option<plugin::PluginManager>,
    /// 사용자 init.lua 기반 Lua hook 엔진. 부팅 시 1회 생성, `~/.tasty/init.lua` 가
    /// 있으면 로드. observe-only — 호스트 동작에는 영향 없음. 초기화 실패 시 None.
    pub(crate) lua_engine: Option<tasty_lua::LuaEngine>,
    /// 현재 열려 있는 `PresetWindow` 의 winit window id. modeless editor 윈도우는
    /// 엔진 전역 단일 인스턴스 — 같은 명령이 다시 들어오면 새 윈도우를 만들지 않고
    /// 이 id 의 윈도우로 포커스만 이동한다.
    pub(crate) preset_window_id: Option<WindowId>,
}

/// State for the modal window shake animation.
pub(crate) struct ModalShake {
    pub(crate) start: std::time::Instant,
    /// Original window position before shake began.
    pub(crate) origin: winit::dpi::PhysicalPosition<i32>,
}

impl App {
    pub(crate) fn new(
        proxy: EventLoopProxy<AppEvent>,
        port_file: Option<String>,
        #[cfg(debug_assertions)] input_simulation_enabled: bool,
    ) -> Self {
        Self {
            engine: engine::Engine::new(proxy.clone(), port_file),
            windows: std::collections::HashMap::new(),
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
            lua_engine: crate::hooks::lua::init_engine(),
            preset_window_id: None,
        }
    }
}
