//! scrollback과 화면을 저장하고, deferred 터미널에 적용할 이전 내용을 준비한다.

use crate::core::CoreState;

pub(super) fn queue_scrollback_for_surface(
    engine: &mut CoreState,
    surface_id: u32,
    persist_id: &str,
) {
    use crate::scrollback_store::ScrollbackRead;
    match crate::scrollback_store::read(persist_id) {
        ScrollbackRead::Loaded(lines) if !lines.is_empty() => {
            engine.pending_scrollback_inject.insert(surface_id, lines);
        }
        ScrollbackRead::Loaded(_) => {}
        ScrollbackRead::Absent => {
            tracing::debug!(
                "scrollback restore: no saved content for surface {surface_id} ({persist_id})"
            );
        }
        // 읽기 오류 원인은 저장소가 기록했다. 여기서는 복원하지 못한 surface를 함께 남긴다.
        ScrollbackRead::Unreadable => {
            tracing::warn!(
                "scrollback restore: cannot load saved content for surface {surface_id} ({persist_id})"
            );
        }
    }
}

/// scrollback 뒤에 현재 화면을 붙여 저장한다. 비어 있거나 저장에 실패하면 None이다.
/// 기존 ID를 재사용하되 같은 capture의 다른 surface가 이미 사용했으면 새 ID로 바꾼다.
/// 새 ID는 쓰기 성공 전에 Terminal store에 기록될 수 있다.
pub(super) fn capture_scrollback_to_disk(
    surface_id: crate::model::SurfaceId,
    store: &mut crate::core::terminal_store::TerminalStore,
    seen_refs: &mut std::collections::HashSet<String>,
) -> Option<String> {
    let terminal = store.get(surface_id)?;
    let lines = collect_capture_lines(terminal);
    if lines.is_empty() {
        return None;
    }
    let persist_id = resolve_capture_persist_id(surface_id, store, seen_refs);
    if let Err(e) = crate::scrollback_store::write(&persist_id, &lines) {
        tracing::warn!(
            "scrollback capture: write failed for surface {} ({persist_id}): {e}",
            surface_id
        );
        return None;
    }
    seen_refs.insert(persist_id.clone());
    Some(persist_id)
}

fn collect_capture_lines(
    terminal: &tasty_terminal::Terminal,
) -> Vec<tasty_terminal::ScrollbackLine> {
    let screen = terminal.screen_snapshot_lines();
    // 줄마다 state mutex를 다시 잡지 않도록 scrollback을 한 번에 읽는다.
    let mut lines = terminal.scrollback_lines_all();
    lines.reserve(screen.len());
    lines.extend(screen);
    lines
}

/// 이번 capture에서 쓰지 않은 기존 ID는 재사용하고 없거나 중복이면 새로 발급한다.
fn resolve_capture_persist_id(
    surface_id: crate::model::SurfaceId,
    store: &mut crate::core::terminal_store::TerminalStore,
    seen_refs: &std::collections::HashSet<String>,
) -> String {
    let existing = store.scrollback_persist_id(surface_id).map(str::to_string);
    if let Some(id) = &existing
        && !seen_refs.contains(id)
    {
        return id.clone();
    }
    if let Some(stale) = existing {
        tracing::warn!(
            "scrollback capture: duplicate persist_id {stale} for surface {} — reassigning fresh",
            surface_id
        );
    }
    let new_id = crate::scrollback_store::new_persist_id();
    store.set_scrollback_persist_id(surface_id, new_id.clone());
    new_id
}
