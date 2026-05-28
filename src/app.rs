//! `App` — winit `ApplicationHandler` 의 본체. 다중 윈도우 + 모달 + 플러그인 매니저 +
//! parked AppState 보관. 메서드는 도메인별 서브모듈로 분산되어 있다.

pub(crate) mod busy;
pub(crate) mod clipboard_record;
pub(crate) mod dispatch;
pub(crate) mod event;
pub(crate) mod event_handler;
pub(crate) mod ipc;
pub(crate) mod modal;
pub(crate) mod persistence;
pub(crate) mod plugin_glue;
pub(crate) mod request_owner;
pub(crate) mod window_access;
pub(crate) mod window_lifecycle;

use std::sync::Arc;

use winit::event_loop::EventLoopProxy;
use winit::window::{Window, WindowId};

use crate::core::Core;
use crate::gpu::GpuState;
use crate::hub::Hub;
use crate::view::View;
use crate::{AppEvent, plugin, state, window};

pub(crate) struct App {
    /// Phase C — 도메인 본체. 마이그레이션 중에는 빈 골격, sub-step 마다 한 필드씩
    /// `EngineState` 에서 이쪽으로 이동한다.
    pub(crate) core: Core,
    /// Phase C — 외부 통신 표면. ipc_server, port_file 보유.
    pub(crate) hub: Hub,
    /// Phase C — GUI 어댑터. proxy, modal/focus 식별자, windows HashMap (예정) 보유.
    pub(crate) view: View,
    /// 모든 윈도우(모달 포함). `view.active_modal_id`로 현재 활성 모달을 식별한다.
    /// 모달도 여기에 들어가며, 모달은 엔진 전역에 최대 1개라는 불변식을 유지한다.
    pub(crate) windows: std::collections::HashMap<WindowId, Box<dyn window::Window>>,
    /// Parked AppStates: preserved when all windows are closed so PTY sessions survive.
    /// Moved into new windows when created, or used directly for IPC.
    /// 윈도우가 파킹되면 그 MainWindow 가 갖고 있던 (AppState, EngineState) 쌍을
    /// 통째로 보관한다. dock reopen / 트레이 복귀 시 짝지어 꺼내 새 MainWindow 에
    /// 재주입한다. engine 만 분리해 보관하면 워크스페이스/설정/scrollback 등이
    /// 사라지므로 반드시 쌍으로 보존한다.
    pub(crate) parked_states: Vec<(state::AppState, crate::engine_state::EngineState)>,
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
    /// Sessionwide engine state — workspaces, settings, hooks, registries.
    /// None until the first MainWindow lifecycle initializes it; Some after.
    pub(crate) engine_state: Option<crate::engine_state::EngineState>,
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
    ) -> anyhow::Result<Self> {
        Ok(Self {
            core: crate::boot::wiring::build_production_core(proxy.clone())?,
            hub: Hub::new(port_file),
            view: View::new(proxy.clone()),
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
            engine_state: None,
            lua_engine: crate::hooks::lua::init_engine(),
            preset_window_id: None,
        })
    }

    /// EngineState 접근자. 부팅 시 `App.engine_state` 에 들어 있다가 첫 MainWindow
    /// 등록 시 그쪽으로 이동한다. 이 헬퍼는 두 위치 중 살아있는 쪽을 찾아 반환한다.
    /// 어디에도 없으면 panic — 호출 경로가 invariant 를 깬 것.
    pub(crate) fn engine_state(&self) -> &crate::engine_state::EngineState {
        if let Some(e) = self.engine_state.as_ref() {
            return e;
        }
        for w in self.windows.values() {
            if let Some(main) = w.as_main() {
                return &main.engine_state;
            }
        }
        panic!("App.engine_state accessed before initialization");
    }

    pub(crate) fn engine_state_mut(&mut self) -> &mut crate::engine_state::EngineState {
        if self.engine_state.is_some() {
            return self.engine_state.as_mut().unwrap();
        }
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                return &mut main.engine_state;
            }
        }
        panic!("App.engine_state accessed before initialization");
    }
}
