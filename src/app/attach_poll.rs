//! 점유된 서버 화면을 주기적으로 갱신한다.
//! 클라이언트 출력은 AttachClientData 이벤트로도 적용하며 여기서는 추가로 확인한다.

use crate::app::App;
use crate::view::RepaintSource;

impl App {
    pub(crate) fn poll_attach_views(&mut self) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.refresh_readonly_views()
            {
                w.mark_dirty_from(RepaintSource::AttachMirror);
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            let _ = engine.refresh_readonly_views(); // dirty 여부 반환값 무시 — parked 는 repaint 안 함.
        }

        self.apply_attach_client_output();
    }
}
