use std::path::PathBuf;

use drag::{DragItem, Image, Options};
use winit::raw_window_handle::HasWindowHandle;

use super::DragResult;

/// 드래그 결과 cell 의 poison 을 보고했는가(첫 1 회만).
///
/// 임계구역은 `Option<DragResult>` 한 칸이라 패닉이 나도 불변식이 성립하고, 읽는 쪽이
/// 메인 스레드라 패닉하면 창 전체가 죽는다 — 복구가 맞다. 조용히 버리면 결과가
/// `Accepted` 로 fallback 해 **사용자가 취소한 드래그가 수락으로 보고된다**.
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
