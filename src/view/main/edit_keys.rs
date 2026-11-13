//! 메인 창의 편집 키(복사·잘라내기·붙여넣기) 판정 입구. host egui 입력칸이 키보드를 가진 동안
//! 바인딩 판정을 egui-winit과 단축키 디스패치보다 먼저 한 번 한다. 보류한 키의 결과는 창의 GPU 상태가
//! 정하고, 넘길 키는 App이 이 창의 평소 이벤트 경로로 재생한다. 계약: `docs/design/policies/key-mapping.md`.

use winit::event::WindowEvent;

use super::MainView;
use crate::adapters::ui::input::edit_key::{EditBindings, EditKey};
use crate::gfx::gpu::edit_keys::{EditKeyOwner, EguiKey};
use crate::runtime::engine_read::EngineRead;
use crate::view::ui::View;

impl MainView {
    /// 편집 키 판정의 결과를 이벤트 경로에 넘긴다. 판정이 키를 가져갔으면 `None`(다시 그리기만
    /// 요청한다), 아니면 이 키가 넘어가는 편집(한 번 판정 표시)이다.
    pub(super) fn edit_key_gate(
        &mut self,
        engine: &EngineRead<'_>,
        event: &WindowEvent,
    ) -> Option<Option<EditKey>> {
        match self.judge_edit_key(engine, event) {
            EguiKey::Taken => {
                self.mark_dirty();
                None
            }
            EguiKey::Pass(edit) => Some(Some(edit)),
            EguiKey::Free => Some(None),
        }
    }

    /// 이벤트 하나의 편집 키 판정. App이 재생하는 키는 이미 판정한 편집으로 넘긴다.
    fn judge_edit_key(&mut self, engine: &EngineRead<'_>, event: &WindowEvent) -> EguiKey {
        if let Some(edit) = self.edit_replay.take() {
            return EguiKey::Pass(edit);
        }
        let target =
            matches!(event, WindowEvent::KeyboardInput { .. }) && self.edit_key_target(engine);
        let window = &self.base.winit;
        let gpu = &mut self.base.gpu;
        if !target && !gpu.edit_keys_busy() {
            return EguiKey::Free;
        }
        gpu.set_edit_bindings(
            target.then(|| EditBindings::from_settings(&engine.settings.keybindings)),
        );
        gpu.judge_egui_key(window, event, EditKeyOwner::Window)
    }

    /// 키를 host egui 입력칸이 받는지. 오버레이(팝업·모달·주소 띠·무대)가 열렸거나, egui로 그리는
    /// 탐색기 surface의 입력칸이 포커스를 가졌을 때다. 탐색기 파일 목록의 편집 키는 기존 디스패치가 맡는다.
    fn edit_key_target(&self, engine: &EngineRead<'_>) -> bool {
        if self.state.keyboard_overlay_open() || self.state.fullscreen_stage_active() {
            return true;
        }
        self.state
            .focused_surface_type(engine)
            .kind_capability(engine, |d| d.consumes_egui_input)
            && crate::shortcuts::focused_explorer_typing(&self.state, engine)
    }
}
