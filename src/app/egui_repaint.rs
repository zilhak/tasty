//! egui가 요청한 repaint를 창에 전달한다. 즉시 요청과 지연 요청의 타이머 발화가 같은 경로를 쓴다.

use std::time::Instant;

use tasty_timer::Precision;
use winit::window::WindowId;

use super::App;
use super::timers::Tick;
use crate::view::RepaintSource;

impl App {
    /// egui viewport는 창마다 ROOT이므로 winit 창 ID로 대상을 찾는다.
    pub(crate) fn deliver_egui_repaint(&mut self, window_id: WindowId) {
        if let Some(w) = self.view.views.get_mut(&window_id) {
            // 애니메이션의 연속 repaint 요청에도 상한을 적용한다.
            w.mark_dirty_from(RepaintSource::EguiAnimation);
        }
        // 셸 설정·부팅 오류 창은 views 밖에 있으므로 hover·애니메이션 요청을 직접 그린다.
        if let Some(w) = [&self.shell_setup_window, &self.boot_error_window]
            .into_iter()
            .flatten()
            .find(|w| w.id() == window_id)
        {
            w.request_redraw();
        }
        // 아직 views에 등록되지 않은 부팅 창의 요청은 여기서 처리하지 않는다.
    }

    /// 지연 요청을 창별 일회성 타이머로 예약한다. 같은 창의 예약은 새 시각으로 바뀐다.
    /// 더 늦은 요청은 repaint 콜백이 미리 거르므로 여기서는 이른 쪽만 들어온다.
    /// 남은 시간을 상대 지연으로 등록하므로 이벤트가 늦게 처리돼도 지난 시각이 남지 않는다.
    pub(crate) fn schedule_egui_repaint(&mut self, window_id: WindowId, at: Instant) {
        let now = Instant::now();
        self.timers.once_after(
            Tick::EguiRepaint(window_id),
            at.saturating_duration_since(now),
            Precision::Strict,
            now,
        );
    }
}
