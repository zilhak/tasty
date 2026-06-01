use std::sync::Arc;

use winit::keyboard::ModifiersState;

use crate::gpu::GpuState;

/// 모든 윈도우 구현체가 공유하는 공통 필드.
///
/// Rust trait은 필드를 가질 수 없으므로, 공통 상태는 이 구조체에 모으고
/// 각 윈도우 struct가 `pub base: ViewBase` 필드로 composition한다.
/// `Window` trait은 `base()`/`base_mut()` 접근자를 요구한다.
pub struct ViewBase {
    pub gpu: GpuState,
    pub winit: Arc<winit::window::Window>,
    pub dirty: bool,
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
            focused: true,
            modifiers: ModifiersState::empty(),
            close_requested: false,
        }
    }
}
