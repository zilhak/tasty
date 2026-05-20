use winit::raw_window_handle::HasWindowHandle;

use super::DragResult;

pub fn start_file_drag(
    _window: &impl HasWindowHandle,
    _paths: &[&str],
) -> Result<DragResult, String> {
    tracing::warn!("File drag not implemented on Linux");
    Ok(DragResult::Cancelled)
}
