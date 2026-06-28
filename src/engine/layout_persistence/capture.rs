//! Live model (`CoreState`) → `SavedLayout` 캡처.
//!
//! `capture_scrollback_to_disk` 가 fresh persist_id 발급 + scrollback 디스크 dump 까지
//! 책임지므로 SeenRefs 추적으로 같은 capture 사이클의 중복 ID 를 self-heal.

use serde_json::json;

use crate::core::CoreState;
use crate::engine::surface_registry::SurfaceKindRegistry;
use crate::model::{Pane, PaneNode, Surface, SurfaceLayout, Tab, Workspace};

use super::LAYOUT_VERSION;
use super::schema::{
    SavedCategory, SavedLayout, SavedPane, SavedPaneNode, SavedSurface, SavedSurfaceLayout,
    SavedTab, SavedWorkspace,
};
use super::scrollback::capture_scrollback_to_disk;

// ── Capture: live model → SavedLayout ──

/// 같은 capture 사이클에서 발급/관찰된 persist_id 들을 추적. 중복 발견 시
/// `capture_scrollback_to_disk` 가 자동으로 fresh ID 를 재발급해 self-heal.
type SeenRefs = std::collections::HashSet<String>;

type MemArc = std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>;

/// Capture cascade 의 공통 인자 묶음. cascade 모든 레벨이 동일한 set 을 가져야
/// 하므로 ctx struct 하나로 묶어 7 개 fn signature 의 보일러플레이트를 제거.
///
/// `seen_refs` 는 mut 누적, 나머지는 immut clone 한 reference 들 — 깊은 트리에
/// 같은 4-tuple 을 매번 풀어 넘기지 않도록 한다.
struct CaptureCtx<'a> {
    registry: &'a SurfaceKindRegistry,
    capture_scrollback: bool,
    memory: &'a MemArc,
    seen_refs: &'a mut SeenRefs,
    terminals: &'a mut crate::core::terminal_store::TerminalStore,
}

impl SavedLayout {
    /// Capture current layout from engine state.
    ///
    /// `&mut CoreState` 인 이유: `capture_scrollback_to_disk` 가 새 persist_id 를
    /// 발급한 경우 `TerminalStore` 의 mapping 을 갱신해야 다음 capture 가 같은
    /// ID 를 재사용한다 (orphan 누적 방지).
    pub fn capture(engine: &mut CoreState, active_workspace: usize) -> Self {
        let registry = engine.surface_registry.clone();
        let capture_scrollback = engine.settings.general.restore_surface_content;
        let memory = engine.memory.clone();
        let categories: Vec<SavedCategory> = engine
            .categories
            .iter()
            .map(|c| SavedCategory {
                id: c.id,
                name: c.name.clone(),
                collapsed: c.collapsed,
            })
            .collect();
        let mut seen_refs = SeenRefs::new();
        let workspaces: Vec<SavedWorkspace> = {
            let CoreState {
                workspaces,
                terminals,
                ..
            } = engine;
            let mut ctx = CaptureCtx {
                registry: registry.as_ref(),
                capture_scrollback,
                memory: &memory,
                seen_refs: &mut seen_refs,
                terminals,
            };
            workspaces
                .iter_mut()
                .map(|ws| SavedWorkspace::capture(ws, &mut ctx))
                .collect()
        };
        Self {
            version: LAYOUT_VERSION,
            workspaces,
            active_workspace,
            categories,
        }
    }
}

impl SavedWorkspace {
    fn capture(ws: &mut Workspace, ctx: &mut CaptureCtx<'_>) -> Self {
        // Find the index of the focused pane BEFORE taking mut borrow of pane_layout.
        let all_ids = ws.pane_layout().all_pane_ids();
        let focused_pane_index = all_ids
            .iter()
            .position(|&id| id == ws.focused_pane)
            .unwrap_or(0);
        let attach_mapping = ws.attach_mapping.clone();
        let category = ws.category;
        let pane_layout = SavedPaneNode::capture(ws.pane_layout_mut(), ctx);
        Self {
            name: ws.name.clone(),
            subtitle: ws.subtitle.clone(),
            description: ws.description.clone(),
            pane_layout,
            focused_pane_index,
            attach_mapping,
            category,
        }
    }
}

impl SavedPaneNode {
    fn capture(node: &mut PaneNode, ctx: &mut CaptureCtx<'_>) -> Self {
        match node {
            PaneNode::Leaf(pane) => SavedPaneNode::Leaf(SavedPane::capture(pane, ctx)),
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => SavedPaneNode::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(SavedPaneNode::capture(first, ctx)),
                second: Box::new(SavedPaneNode::capture(second, ctx)),
            },
        }
    }
}

impl SavedPane {
    fn capture(pane: &mut Pane, ctx: &mut CaptureCtx<'_>) -> Self {
        let active_tab = pane.active_tab;
        let tabs = pane
            .tabs
            .iter_mut()
            .map(|t| SavedTab::capture(t, ctx))
            .collect();
        Self { tabs, active_tab }
    }
}

impl SavedTab {
    fn capture(tab: &mut Tab, ctx: &mut CaptureCtx<'_>) -> Self {
        let name = tab.name.clone();
        let explicit_name = tab.explicit_name.clone();
        let surface = if tab.is_split() {
            SavedSurfaceLayout::capture_layout(tab.layout_mut(), ctx)
        } else {
            SavedSurfaceLayout::Leaf(SavedSurface::capture_surface(tab.surface_mut(), ctx))
        };
        Self {
            name,
            explicit_name,
            surface,
        }
    }
}

impl SavedSurfaceLayout {
    fn capture_layout(layout: &mut SurfaceLayout, ctx: &mut CaptureCtx<'_>) -> Self {
        match layout {
            SurfaceLayout::Leaf(surface) => {
                SavedSurfaceLayout::Leaf(SavedSurface::capture_surface(surface.as_mut(), ctx))
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
                first: Box::new(SavedSurfaceLayout::capture_layout(first, ctx)),
                second: Box::new(SavedSurfaceLayout::capture_layout(second, ctx)),
            },
        }
    }
}

/// Deferred terminal 의 scrollback 복원을 큐에 적재. PTY 가 spawn 되는 시점에
/// `apply_pending_scrollback_inject` 가 꺼내 inject 한다.
impl SavedSurface {
    fn capture_surface(surface: &mut dyn Surface, ctx: &mut CaptureCtx<'_>) -> Self {
        let registry = ctx.registry;
        let capture_scrollback = ctx.capture_scrollback;
        let memory = ctx.memory;
        if let Some(ts) = surface
            .as_any()
            .downcast_ref::<crate::model::TerminalSurface>()
        {
            let surface_id = ts.id;
            let restore_command = {
                let mut guard = match memory.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                crate::surface_meta::SurfaceMetaStore::get(
                    &mut *guard,
                    surface_id,
                    "restore.command",
                )
            };
            let cwd = ctx
                .terminals
                .get(surface_id)
                .and_then(|t| t.get_cwd())
                .map(|p| p.to_string_lossy().to_string());
            let scrollback_ref = if capture_scrollback {
                capture_scrollback_to_disk(surface_id, ctx.terminals, ctx.seen_refs)
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
        // PTY 가 아직 안 떠 있어 TerminalSurface downcast 가 None 이다.
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
            // deferred surface 복원 명령의 권위 출처는 `DeferredSpawn.restore_command`
            // (복원 시 layout 에서 이관됨) 뿐이다. `surface_meta[id]` 로 fallback 하면
            // 재사용된 id 에 남은 stale `claude -r …` 를 주워 담을 수 있으므로 읽지 않는다.
            let restore_command = es
                .deferred_spawn
                .as_ref()
                .and_then(|s| s.restore_command.clone());
            // 옵션 on 일 때만 scrollback_ref 를 다음 capture 까지 유지한다.
            // (옵션 off 면 다음 capture 때 디스크 쓰기를 스킵하므로 ref 도 의미 없음 →
            //  파일은 startup GC 가 청소한다.)
            let scrollback_ref = if capture_scrollback {
                let stored = es
                    .deferred_spawn
                    .as_ref()
                    .and_then(|s| s.scrollback_persist_id.clone());
                match stored {
                    Some(existing) if !ctx.seen_refs.contains(&existing) => {
                        ctx.seen_refs.insert(existing.clone());
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
                        ctx.seen_refs.insert(new_id.clone());
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
        if let Some(def) = registry.get(&kind)
            && let Some(data) = (def.snapshot)(&*surface)
        {
            return SavedSurface::Generic { kind, data };
        }
        // snapshot 함수가 None을 반환했거나 registry에 없는 kind면 Empty로 fallback.
        SavedSurface::Generic {
            kind: "empty".into(),
            data: json!({}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::engine::surface_registry::SurfaceKindRegistry;

    /// Fix B 회귀: deferred surface 의 `DeferredSpawn.restore_command` 가 None 이면,
    /// surface_meta 에 같은 id 의 (재사용된 stale) restore.command 가 있어도 capture 는
    /// 이를 무시하고 None 을 저장해야 한다.
    #[test]
    fn deferred_capture_ignores_surface_meta_restore_command() {
        let surface_id = 7u32;

        // memory.db 에 stale restore.command 를 심는다 (이전 실행 잔재 모사).
        let mem: MemArc = Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        {
            let mut guard = mem.lock().unwrap();
            crate::surface_meta::SurfaceMetaStore::set(
                &mut *guard,
                surface_id,
                "restore.command",
                "claude -r STALE-FROM-PREVIOUS-RUN",
            )
            .unwrap();
        }

        // restore_command = None 인 deferred placeholder.
        let waker: tasty_terminal::Waker = Arc::new(|| {});
        let spawn = crate::model::DeferredSpawn {
            shell: None,
            shell_args: Vec::new(),
            cols: 80,
            rows: 24,
            waker,
            working_dir: None,
            restore_command: None,
            scrollback_persist_id: None,
        };
        let mut es = crate::model::EmptySurface::new_deferred(surface_id, spawn);

        let registry = SurfaceKindRegistry::new();
        let mut seen_refs = SeenRefs::new();
        let mut terminals = crate::core::terminal_store::TerminalStore::new();
        let mut ctx = CaptureCtx {
            registry: &registry,
            capture_scrollback: false,
            memory: &mem,
            seen_refs: &mut seen_refs,
            terminals: &mut terminals,
        };

        let saved = SavedSurface::capture_surface(&mut es, &mut ctx);
        match saved {
            SavedSurface::Terminal {
                restore_command, ..
            } => assert_eq!(
                restore_command, None,
                "deferred capture 는 surface_meta fallback 으로 stale 을 주워오면 안 된다"
            ),
            _ => panic!("expected SavedSurface::Terminal"),
        }
    }
}
