use std::sync::Arc;

use winit::keyboard::ModifiersState;

use crate::gpu::GpuState;
use crate::view::repaint::RepaintGate;

/// 모든 창의 공통 상태. 각 창이 보관하고 View 접근자로 제공한다.
pub struct ViewBase {
    pub gpu: GpuState,
    pub winit: Arc<winit::window::Window>,
    pub dirty: bool,
    /// 리페인트 요청 상한(창별 독립). 근거·분류는 [`crate::view::repaint`] 모듈 문서.
    pub repaint: RepaintGate,
    pub focused: bool,
    pub modifiers: ModifiersState,
    pub close_requested: bool,
}

impl ViewBase {
    pub fn new(gpu: GpuState, winit: Arc<winit::window::Window>) -> Self {
        Self {
            gpu,
            winit,
            dirty: true,
            repaint: RepaintGate::new(),
            focused: true,
            modifiers: ModifiersState::empty(),
            close_requested: false,
        }
    }

    /// 프레임을 그릴 때 dirty 해제와 다시 그리기 시간 기준 갱신을 함께 처리한다.
    pub fn begin_frame(&mut self) {
        self.dirty = false;
        self.repaint.note_present(std::time::Instant::now());
    }
}
