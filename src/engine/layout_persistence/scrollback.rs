//! Surface scrollback 의 디스크 dump (capture) + deferred restore queue.

use crate::core::CoreState;

pub(super) fn queue_scrollback_for_surface(
    engine: &mut CoreState,
    surface_id: u32,
    persist_id: &str,
) {
    match crate::scrollback_store::read(persist_id) {
        Some(lines) if !lines.is_empty() => {
            engine.pending_scrollback_inject.insert(surface_id, lines);
        }
        Some(_) => {}
        None => {
            tracing::debug!(
                "scrollback restore: file missing for surface {surface_id} ({persist_id})"
            );
        }
    }
}

/// `TerminalSurface` 의 scrollback + 현재 화면(visible) 을 묶어
/// `~/.tasty/scrollback/<id>.bin` 으로 덤프하고 `<id>` 를 반환. persist_id 는
/// `TerminalSurface.scrollback_persist_id` 필드에 in-memory 로 보관되며 다음
/// capture 가 같은 ID 를 재사용한다 (orphan 누적 방지). 세션 간 stale meta
/// 가 다른 surface 에 상속되는 사고가 원천 차단된다.
///
/// 화면 라인은 scrollback 의 뒤에 이어 붙인다 → 복원 시 위로 스크롤하면
/// [이전 scrollback → 이전 화면 → 새 prompt] 순으로 보인다.
///
/// `seen_refs` 는 같은 capture 사이클에서 이미 사용된 persist_id 집합. 충돌
/// 발견 시 fresh ID 를 발급해 self-heal 한다 — 과거에 layout.json 에 중복이
/// 들어간 적이 있어도 다음 첫 capture 가 정리한다.
///
/// 실패하거나 (scrollback + screen) 양쪽 모두 비어 있으면 `None`.
pub(super) fn capture_scrollback_to_disk(
    ts: &mut crate::model::TerminalSurface,
    seen_refs: &mut std::collections::HashSet<String>,
) -> Option<String> {
    let total = ts.terminal.scrollback_len();
    let screen = ts.terminal.screen_snapshot_lines();
    if total == 0 && screen.is_empty() {
        return None;
    }
    let mut lines = Vec::with_capacity(total + screen.len());
    for i in 0..total {
        if let Some(line) = ts.terminal.scrollback_line_full(i) {
            lines.push(line);
        }
    }
    lines.extend(screen);
    if lines.is_empty() {
        return None;
    }
    // 중복 가드: 다른 surface 가 같은 사이클에서 이미 쓴 ID 면 fresh 로 교체.
    let persist_id = match ts.scrollback_persist_id.clone() {
        Some(existing) if !seen_refs.contains(&existing) => existing,
        Some(stale) => {
            tracing::warn!(
                "scrollback capture: duplicate persist_id {stale} for surface {} — reassigning fresh",
                ts.id
            );
            let new_id = crate::scrollback_store::new_persist_id();
            ts.scrollback_persist_id = Some(new_id.clone());
            new_id
        }
        None => {
            let new_id = crate::scrollback_store::new_persist_id();
            ts.scrollback_persist_id = Some(new_id.clone());
            new_id
        }
    };
    if let Err(e) = crate::scrollback_store::write(&persist_id, &lines) {
        tracing::warn!(
            "scrollback capture: write failed for surface {} ({persist_id}): {e}",
            ts.id
        );
        return None;
    }
    seen_refs.insert(persist_id.clone());
    Some(persist_id)
}
