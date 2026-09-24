use std::path::PathBuf;

use drag::{DragItem, Image, Options};
use winit::raw_window_handle::HasWindowHandle;

use super::DragResult;

/// poison 로그를 한 번만 남긴다. 기존 결과를 버리면 기본 Accepted로 잘못 바뀔 수 있어 보존한다.
static DRAG_RESULT_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
const DRAG_RESULT_WHAT: &str = "file drag result cell";

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
            *crate::poison::recover_mutex(
                result_clone.lock(),
                DRAG_RESULT_WHAT,
                &DRAG_RESULT_POISONED,
            ) = Some(drag_result);
        },
        Options::default(),
    )
    .map_err(|e| e.to_string())?;

    let outcome =
        (*crate::poison::recover_mutex(result.lock(), DRAG_RESULT_WHAT, &DRAG_RESULT_POISONED))
            .unwrap_or(DragResult::Accepted);
    Ok(outcome)
}
