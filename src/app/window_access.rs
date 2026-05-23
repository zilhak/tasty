//! 활성 윈도우 접근 헬퍼.
//!
//! IPC / 키보드 라우팅의 일반 대상은 모달이 아닌 `MainWindow`. 모달 활성 여부와는
//! 별개로 `engine.focused_window_id` 로 추적되는 윈도우만 반환한다.

use crate::app::App;
use crate::window;

impl App {
    /// Get the focused main window, if any.
    /// 모달이 아닌 MainWindow만 반환한다 — IPC/키보드 라우팅의 일반적 대상.
    pub(crate) fn focused_window(&self) -> Option<&window::main::MainWindow> {
        self.engine
            .focused_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|w| w.as_main())
    }

    pub(crate) fn focused_window_mut(&mut self) -> Option<&mut window::main::MainWindow> {
        self.engine
            .focused_window_id
            .and_then(|id| self.windows.get_mut(&id))
            .and_then(|w| w.as_main_mut())
    }

    /// 모든 MainWindow를 순회. 모달은 제외된다.
    pub(crate) fn main_windows_iter_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut window::main::MainWindow> {
        self.windows.values_mut().filter_map(|w| w.as_main_mut())
    }

    /// 살아있는 EngineState 중 하나(아무거나)를 참조로 반환. windows main → parked
    /// 순으로 찾는다. 두 번째 main window 생성 시 첫 engine 의 Arc 들 (surface_registry /
    /// file_format / file_handler / preset_store / identify_worker / approval_store /
    /// telemetry_seq / anomaly_detector / agent_seq) 을 공유시키기 위해 사용.
    pub(crate) fn any_main_engine(&self) -> Option<&crate::engine_state::EngineState> {
        for w in self.windows.values() {
            if let Some(m) = w.as_main() {
                return Some(&m.engine_state);
            }
        }
        self.parked_states.first().map(|(_, e)| e)
    }
}
