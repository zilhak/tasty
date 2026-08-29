use std::sync::Arc;

use winit::keyboard::ModifiersState;

use crate::gpu::GpuState;
use crate::view::repaint::RepaintGate;

/// 모든 윈도우 구현체가 공유하는 공통 필드.
///
/// Rust trait은 필드를 가질 수 없으므로, 공통 상태는 이 구조체에 모으고
/// 각 윈도우 struct가 `pub base: ViewBase` 필드로 composition한다.
/// `Window` trait은 `base()`/`base_mut()` 접근자를 요구한다.
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

    /// 이번 프레임을 그리기로 확정한 지점 — `dirty` 를 내리고 상한 게이트의 기준
    /// 시각을 갱신한다. 둘이 갈라지면 상한이 실제 프레임 cadence 를 못 따라가
    /// 게이트가 무의미해지므로 한 호출로 묶는다.
    pub fn begin_frame(&mut self) {
        self.dirty = false;
        self.repaint.note_present(std::time::Instant::now());
    }
}
