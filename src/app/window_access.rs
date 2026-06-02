//! 활성 윈도우 접근 헬퍼.
//!
//! IPC / 키보드 라우팅의 일반 대상은 모달이 아닌 `MainWindow`. 모달 활성 여부와는
//! 별개로 `view.focused_window_id` 로 추적되는 윈도우만 반환한다.

use winit::window::WindowId;

use crate::app::App;
use crate::view;

impl App {
    /// Get the focused main window, if any.
    /// 모달이 아닌 MainWindow만 반환한다 — IPC/키보드 라우팅의 일반적 대상.
    pub(crate) fn focused_window(&self) -> Option<&view::main::MainWindow> {
        self.view
            .focused_window_id
            .and_then(|id| self.view.windows.get(&id))
            .and_then(|w| w.as_main())
    }

    pub(crate) fn focused_window_mut(&mut self) -> Option<&mut view::main::MainWindow> {
        self.view
            .focused_window_id
            .and_then(|id| self.view.windows.get_mut(&id))
            .and_then(|w| w.as_main_mut())
    }

    /// 모든 MainWindow를 순회. 모달은 제외된다.
    pub(crate) fn main_windows_iter_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut view::main::MainWindow> {
        self.view
            .windows
            .values_mut()
            .filter_map(|w| w.as_main_mut())
    }

    /// 살아있는 CoreState 중 하나(아무거나)를 참조로 반환. windows main → parked
    /// 순으로 찾는다. 두 번째 main window 생성 시 첫 engine 의 Arc 들 (surface_registry /
    /// file_format / file_handler / preset_store / identify_worker / approval_store /
    /// telemetry_seq / anomaly_detector / agent_seq) 을 공유시키기 위해 사용.
    pub(crate) fn any_main_engine(&self) -> Option<&crate::core::CoreState> {
        for w in self.view.windows.values() {
            if let Some(m) = w.as_main() {
                return Some(&m.core_state);
            }
        }
        self.parked_states.first().map(|(_, e)| e)
    }

    /// Surface 를 가진 MainWindow 의 WindowId 를 반환. windows main 순회 후 못 찾으면
    /// None (parked 는 별도로 fallback 처리).
    pub(crate) fn find_main_with_surface(&self, surface_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.windows {
            if let Some(m) = w.as_main() {
                if m.core_state.has_surface(surface_id) {
                    return Some(*wid);
                }
            }
        }
        None
    }

    /// Workspace 를 가진 MainWindow 의 WindowId 를 반환.
    pub(crate) fn find_main_with_workspace(&self, workspace_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.windows {
            if let Some(m) = w.as_main() {
                if m.core_state.has_workspace(workspace_id) {
                    return Some(*wid);
                }
            }
        }
        None
    }

    /// Pane 을 가진 MainWindow 의 WindowId 를 반환.
    pub(crate) fn find_main_with_pane(&self, pane_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.windows {
            if let Some(m) = w.as_main() {
                if m.core_state.has_pane(pane_id) {
                    return Some(*wid);
                }
            }
        }
        None
    }
}
