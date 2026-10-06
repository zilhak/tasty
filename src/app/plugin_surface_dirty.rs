//! 플러그인 surface의 화면 갱신 요청을 창의 다시 그리기로 옮긴다.

use super::App;
use crate::view::ui::View;

impl App {
    /// 무입력 변경 요청(SurfaceInvalidated)은 다음 렌더에서 다시 전달하도록 표시하고,
    /// 새로 받은 frame은 그 surface가 보이는 창을 다시 그린다.
    pub(super) fn mark_plugin_surfaces_dirty(&mut self) {
        let (invalidated, fresh) = self
            .plugin_manager
            .as_mut()
            .map(|mgr| {
                (
                    mgr.take_invalidated_surfaces(),
                    mgr.take_fresh_frame_surfaces(),
                )
            })
            .unwrap_or_default();
        if invalidated.is_empty() && fresh.is_empty() {
            return;
        }
        for (_, main, engine) in self.engines_mut().window_pairs() {
            let engine = engine.read();
            // any는 첫 true에서 멈추므로 모든 surface를 표시할 수 없다.
            let mut touched = false;
            for &sid in &invalidated {
                if main.mark_surface_invalidated(&engine, sid) {
                    touched = true;
                }
            }
            // 창은 plugin의 frame이 도착하기 전에 그렸을 수 있다. 새 frame은 다시 그려야 보인다.
            if fresh
                .iter()
                .any(|&sid| main.is_surface_visible(&engine, sid))
            {
                touched = true;
            }
            if touched {
                main.mark_dirty();
            }
        }
    }
}
