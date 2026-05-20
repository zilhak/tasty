use std::path::PathBuf;

use drag::{DragItem, Image, Options};
use winit::raw_window_handle::HasWindowHandle;

use super::DragResult;

pub fn start_file_drag(
    window: &impl HasWindowHandle,
    paths: &[&str],
) -> Result<DragResult, String> {
    let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let item = DragItem::Files(path_bufs);

    let result = std::sync::Arc::new(std::sync::Mutex::new(None));
    let result_clone = result.clone();

    drag::start_drag(
        window,
        item,
        Image::Raw(Vec::new()),
        move |outcome, _cursor_pos| {
            let drag_result = match outcome {
                drag::DragResult::Dropped => DragResult::Accepted,
                drag::DragResult::Cancel => DragResult::Cancelled,
            };
            if let Ok(mut guard) = result_clone.lock() {
                *guard = Some(drag_result);
            }
        },
        Options::default(),
    )
    .map_err(|e| e.to_string())?;

    let outcome = result
        .lock()
        .ok()
        .and_then(|g| *g)
        .unwrap_or(DragResult::Accepted);
    Ok(outcome)
}
