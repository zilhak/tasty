use std::sync::Arc;

use crate::gpu::GpuState;

/// Per-View OS/GPU resource owner, composed with common display values.
pub struct ViewBase {
    pub gpu: GpuState,
    pub winit: Arc<winit::window::Window>,
    pub(crate) state: crate::view::state::ViewState,
}

impl ViewBase {
    pub fn new(gpu: GpuState, winit: Arc<winit::window::Window>) -> Self {
        Self {
            gpu,
            winit,
            state: Default::default(),
        }
    }

    /// 사용자 입력에 따른 redraw 요청. 창 View를 빌리지 않고 이 상태만 가진 경로가 쓴다.
    pub fn mark_dirty(&mut self) {
        self.mark_dirty_from(crate::view::RepaintSource::Interactive);
    }

    /// 요청 원인에 따라 redraw를 미루되 dirty는 즉시 설정한다.
    /// 미룬 시각은 about_to_wait에서 WaitUntil로 예약한다.
    pub fn mark_dirty_from(&mut self, source: crate::view::RepaintSource) {
        self.state.dirty = true;
        if self
            .state
            .repaint
            .admit(source, std::time::Instant::now(), &self.winit)
        {
            self.winit.request_redraw();
        }
    }

    /// 프레임을 그릴 때 dirty 해제와 다시 그리기 시간 기준 갱신을 함께 처리한다.
    pub fn begin_frame(&mut self) {
        self.state.dirty = false;
        self.state.repaint.note_present(std::time::Instant::now());
    }
}
