//! macOS 창 단위 오버레이 host. 아직 구현하지 않아 만들지 않는다. 구현은 이 파일의
//! `create` 만 바꾸며 계약은 상위 모듈 `overlay_host` 와 docs/design/systems/webview.md 를 따른다.

use std::sync::Arc;

use winit::window::Window;

use super::{OverlayHostError, OverlayNativeHost};

pub(super) fn create(
    _window: &Arc<Window>,
) -> Result<Box<dyn OverlayNativeHost>, OverlayHostError> {
    Err(OverlayHostError::Unsupported)
}
