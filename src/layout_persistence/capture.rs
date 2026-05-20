//! Live model (`EngineState`) → `SavedLayout` 캡처.
//!
//! `capture_scrollback_to_disk` 가 fresh persist_id 발급 + scrollback 디스크 dump 까지
//! 책임지므로 SeenRefs 추적으로 같은 capture 사이클의 중복 ID 를 self-heal.

use serde_json::json;

use crate::engine_state::EngineState;
use crate::model::{Pane, PaneNode, Surface, SurfaceLayout, Tab, Workspace};
use crate::surface_registry::SurfaceKindRegistry;

use super::LAYOUT_VERSION;
use super::schema::{
    SavedLayout, SavedPane, SavedPaneNode, SavedSurface, SavedSurfaceLayout, SavedTab,
    SavedWorkspace,
};
use super::scrollback::capture_scrollback_to_disk;

// ── Capture: live model → SavedLayout ──

/// 같은 capture 사이클에서 발급/관찰된 persist_id 들을 추적. 중복 발견 시
/// `capture_scrollback_to_disk` 가 자동으로 fresh ID 를 재발급해 self-heal.
type SeenRefs = std::collections::HashSet<String>;

impl SavedLayout {
    /// Capture current layout from engine state.
    ///
    /// `&mut EngineState` 인 이유: `capture_scrollback_to_disk` 가 새 persist_id 를
    /// 발급한 경우 surface 인스턴스의 `scrollback_persist_id` 필드에 기록해야
    /// 다음 capture 가 같은 ID 를 재사용한다 (orphan 누적 방지).
    pub fn capture(engine: &mut EngineState, active_workspace: usize) -> Self {
        let registry = engine.surface_registry.clone();
        let capture_scrollback = engine.settings.general.restore_terminal_content;
        let mut seen_refs = SeenRefs::new();
        let workspaces: Vec<SavedWorkspace> = engine
            .workspaces
            .iter_mut()
            .map(|ws| {
                SavedWorkspace::capture(ws, registry.as_ref(), capture_scrollback, &mut seen_refs)
            })
            .collect();
        Self {
            version: LAYOUT_VERSION,
            workspaces,
            active_workspace,
        }
    }
}

impl SavedWorkspace {
    fn capture(
        ws: &mut Workspace,
        registry: &SurfaceKindRegistry,
        capture_scrollback: bool,
        seen_refs: &mut SeenRefs,
    ) -> Self {
        // Find the index of the focused pane BEFORE taking mut borrow of pane_layout.
        let all_ids = ws.pane_layout().all_pane_ids();
        let focused_pane_index = all_ids
            .iter()
            .position(|&id| id == ws.focused_pane)
            .unwrap_or(0);
        let pane_layout = SavedPaneNode::capture(
            ws.pane_layout_mut(),
            registry,
            capture_scrollback,
            seen_refs,
        );
        Self {
            name: ws.name.clone(),
            subtitle: ws.subtitle.clone(),
            description: ws.description.clone(),
            pane_layout,
            focused_pane_index,
        }
    }
}

impl SavedPaneNode {
    fn capture(
        node: &mut PaneNode,
        registry: &SurfaceKindRegistry,
        capture_scrollback: bool,
        seen_refs: &mut SeenRefs,
    ) -> Self {
        match node {
            PaneNode::Leaf(pane) => SavedPaneNode::Leaf(SavedPane::capture(
                pane,
                registry,
                capture_scrollback,
                seen_refs,
            )),
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => SavedPaneNode::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(SavedPaneNode::capture(
                    first,
                    registry,
                    capture_scrollback,
                    seen_refs,
                )),
                second: Box::new(SavedPaneNode::capture(
                    second,
                    registry,
                    capture_scrollback,
                    seen_refs,
                )),
            },
        }
    }
}

impl SavedPane {
    fn capture(
        pane: &mut Pane,
        registry: &SurfaceKindRegistry,
        capture_scrollback: bool,
        seen_refs: &mut SeenRefs,
    ) -> Self {
        let active_tab = pane.active_tab;
        let tabs = pane
            .tabs
            .iter_mut()
            .map(|t| SavedTab::capture(t, registry, capture_scrollback, seen_refs))
            .collect();
        Self { tabs, active_tab }
    }
}

impl SavedTab {
    fn capture(
        tab: &mut Tab,
        registry: &SurfaceKindRegistry,
        capture_scrollback: bool,
        seen_refs: &mut SeenRefs,
    ) -> Self {
        let name = tab.name.clone();
        let explicit_name = tab.explicit_name.clone();
        let surface = if tab.is_split() {
            SavedSurfaceLayout::capture_layout(
                tab.layout_mut(),
                registry,
                capture_scrollback,
                seen_refs,
            )
        } else {
            SavedSurfaceLayout::Leaf(SavedSurface::capture_surface(
                tab.surface_mut(),
                registry,
                capture_scrollback,
                seen_refs,
            ))
        };
        Self {
            name,
            explicit_name,
            surface,
        }
    }
}

impl SavedSurfaceLayout {
    fn capture_layout(
        layout: &mut SurfaceLayout,
        registry: &SurfaceKindRegistry,
        capture_scrollback: bool,
        seen_refs: &mut SeenRefs,
    ) -> Self {
        match layout {
            SurfaceLayout::Leaf(surface) => {
                SavedSurfaceLayout::Leaf(SavedSurface::capture_surface(
                    surface.as_mut(),
                    registry,
                    capture_scrollback,
                    seen_refs,
                ))
            }
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => SavedSurfaceLayout::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(SavedSurfaceLayout::capture_layout(
                    first,
                    registry,
                    capture_scrollback,
                    seen_refs,
                )),
                second: Box::new(SavedSurfaceLayout::capture_layout(
                    second,
                    registry,
                    capture_scrollback,
                    seen_refs,
                )),
            },
        }
    }
}

/// Deferred terminal 의 scrollback 복원을 큐에 적재. PTY 가 spawn 되는 시점에
/// `apply_pending_scrollback_inject` 가 꺼내 inject 한다.
impl SavedSurface {
    fn capture_surface(
        surface: &mut dyn Surface,
        registry: &SurfaceKindRegistry,
        capture_scrollback: bool,
        seen_refs: &mut SeenRefs,
    ) -> Self {
        if let Some(ts) = surface.as_terminal_surface_mut() {
            let restore_command =
                crate::surface_meta::SurfaceMetaStore::get(ts.id, "restore.command");

            let cwd = ts
                .terminal
                .get_cwd()
                .map(|p| p.to_string_lossy().to_string());
            let scrollback_ref = if capture_scrollback {
                capture_scrollback_to_disk(ts, seen_refs)
            } else {
                None
            };

            return SavedSurface::Terminal {
                cwd,
                restore_command,
                scrollback_ref,
            };
        }
        // 비활성 탭의 deferred EmptySurface 는 외부로는 Terminal 역할이지만
        // PTY 가 아직 안 떠 있어 `as_terminal_surface()` 가 None 이다.
        // `DeferredSpawn` 자체가 들고 있는 cwd / restore_command / persist_id
        // 를 그대로 옮겨 round-trip 한다.
        if let Some(es) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::EmptySurface>()
            && es.is_deferred()
        {
            let surface_id = es.id;
            let cwd = es
                .deferred_spawn
                .as_ref()
                .and_then(|s| s.working_dir.as_ref())
                .map(|p| p.to_string_lossy().to_string());
            let restore_command = es
                .deferred_spawn
                .as_ref()
                .and_then(|s| s.restore_command.clone())
                .or_else(|| crate::surface_meta::SurfaceMetaStore::get(es.id, "restore.command"));
            // 옵션 on 일 때만 scrollback_ref 를 다음 capture 까지 유지한다.
            // (옵션 off 면 다음 capture 때 디스크 쓰기를 스킵하므로 ref 도 의미 없음 →
            //  파일은 startup GC 가 청소한다.)
            let scrollback_ref = if capture_scrollback {
                let stored = es
                    .deferred_spawn
                    .as_ref()
                    .and_then(|s| s.scrollback_persist_id.clone());
                match stored {
                    Some(existing) if !seen_refs.contains(&existing) => {
                        seen_refs.insert(existing.clone());
                        Some(existing)
                    }
                    Some(stale) => {
                        // PTY 가 안 떠 있어 직접 dump 불가 → 디스크 내용을 fresh
                        // 파일로 복사하고 새 ID 를 발급. 한쪽이 라이브 surface 였다면
                        // 그쪽은 이미 자기 데이터로 stale.bin 을 덮어쓴 후다.
                        let new_id = crate::scrollback_store::new_persist_id();
                        if let Some(lines) = crate::scrollback_store::read(&stale) {
                            if let Err(e) = crate::scrollback_store::write(&new_id, &lines) {
                                tracing::warn!(
                                    "scrollback capture(deferred): copy {stale} → {new_id} failed for surface {surface_id}: {e}"
                                );
                            } else {
                                tracing::warn!(
                                    "scrollback capture(deferred): duplicate persist_id {stale} on surface {surface_id} → reassigned to {new_id}"
                                );
                            }
                        }
                        if let Some(spawn) = es.deferred_spawn.as_mut() {
                            spawn.scrollback_persist_id = Some(new_id.clone());
                        }
                        seen_refs.insert(new_id.clone());
                        Some(new_id)
                    }
                    None => None,
                }
            } else {
                None
            };
            return SavedSurface::Terminal {
                cwd,
                restore_command,
                scrollback_ref,
            };
        }
        let kind = surface.kind().to_string();
        if let Some(def) = registry.get(&kind) {
            if let Some(data) = (def.snapshot)(&*surface) {
                return SavedSurface::Generic { kind, data };
            }
        }
        // snapshot 함수가 None을 반환했거나 registry에 없는 kind면 Empty로 fallback.
        SavedSurface::Generic {
            kind: "empty".into(),
            data: json!({}),
        }
    }
}
