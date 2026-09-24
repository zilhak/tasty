//! 현재 레이아웃과 surface 복원 정보를 저장용 데이터로 만든다.

use serde_json::json;

use crate::core::CoreState;
use crate::core::surface_registry::SurfaceKindRegistry;
use crate::model::{Deferred, Pane, PaneNode, Surface, SurfaceLayout, Tab, Workspace};

use super::LAYOUT_VERSION;
use super::schema::{
    SavedCategory, SavedLayout, SavedPane, SavedPaneNode, SavedSurface, SavedSurfaceLayout,
    SavedTab, SavedWorkspace,
};
use super::scrollback::capture_scrollback_to_disk;

/// 이번 capture에서 사용한 scrollback ID. 중복이면 새 ID를 발급한다.
type SeenRefs = std::collections::HashSet<String>;

type MemArc = std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>;

/// 트리 전체가 같은 ID 집합과 저장 설정을 사용하도록 전달하는 공통 상태.
struct CaptureCtx<'a> {
    registry: &'a SurfaceKindRegistry,
    capture_scrollback: bool,
    memory: &'a MemArc,
    seen_refs: &'a mut SeenRefs,
    terminals: &'a mut crate::core::terminal_store::TerminalStore,
}

impl SavedLayout {
    /// 새 scrollback ID를 Terminal store에도 기록하므로 engine을 변경할 수 있다.
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
        // mirror는 원격 세션 없이 복원할 수 없으므로 저장하지 않는다. 제외 후 활성 인덱스도 맞춘다.
        let active_workspace = engine
            .workspaces
            .iter()
            .take(active_workspace)
            .filter(|ws| !ws.mirror)
            .count();
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
                .filter(|ws| !ws.mirror)
                .map(|ws| SavedWorkspace::capture(ws, &mut ctx))
                .collect()
        };
        let active_workspace = active_workspace.min(workspaces.len().saturating_sub(1));
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

/// PTY 생성 후 적용할 scrollback을 대기열에 넣는다.
impl SavedSurface {
    fn capture_surface(surface: &mut dyn Surface, ctx: &mut CaptureCtx<'_>) -> Self {
        if let Some(ts) = surface
            .as_any()
            .downcast_ref::<crate::model::TerminalSurface>()
        {
            return Self::capture_terminal_surface(ts, ctx);
        }
        // 아직 PTY가 없는 terminal과 plugin placeholder도 원래 복원 정보를 유지한다.
        if let Some(es) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::EmptySurface>()
        {
            // enum으로 종류를 구별해 plugin placeholder를 terminal로 잘못 저장하지 않게 한다.
            match &es.deferred {
                Some(Deferred::Plugin(p)) => {
                    let saved = SavedSurface::Generic {
                        kind: p.kind.clone(),
                        data: p.snapshot.clone(),
                    };
                    return saved;
                }
                Some(Deferred::Terminal(_)) => {
                    return Self::capture_deferred_surface(es, ctx);
                }
                None => {}
            }
        }
        Self::capture_generic_surface(&*surface, ctx.registry)
    }

    fn capture_terminal_surface(
        ts: &crate::model::TerminalSurface,
        ctx: &mut CaptureCtx<'_>,
    ) -> Self {
        let surface_id = ts.id;
        let restore_command = {
            let mut guard = crate::poison::recover_mutex(
                ctx.memory.lock(),
                crate::core::MEMORY_WHAT,
                &crate::core::MEMORY_POISONED,
            );
            crate::surface_meta::SurfaceMetaStore::get(&mut *guard, surface_id, "restore.command")
        };
        let cwd = ctx
            .terminals
            .get(surface_id)
            .and_then(|t| t.get_cwd())
            .map(|p| p.to_string_lossy().to_string());
        let scrollback_ref = if ctx.capture_scrollback {
            capture_scrollback_to_disk(surface_id, ctx.terminals, ctx.seen_refs)
        } else {
            None
        };

        SavedSurface::Terminal {
            cwd,
            restore_command,
            scrollback_ref,
        }
    }

    // deferred의 복원 명령은 DeferredSpawn에서만 읽는다. 재사용된 ID의 오래된 메타데이터를 가져오지 않는다.
    fn capture_deferred_surface(
        es: &mut crate::model::EmptySurface,
        ctx: &mut CaptureCtx<'_>,
    ) -> Self {
        let surface_id = es.id;
        let cwd = es
            .deferred_spawn()
            .and_then(|s| s.working_dir.as_ref())
            .map(|p| p.to_string_lossy().to_string());
        let restore_command = es.deferred_spawn().and_then(|s| s.restore_command.clone());
        let scrollback_ref = if ctx.capture_scrollback {
            Self::resolve_deferred_scrollback_ref(es, ctx, surface_id)
        } else {
            None
        };
        SavedSurface::Terminal {
            cwd,
            restore_command,
            scrollback_ref,
        }
    }

    fn resolve_deferred_scrollback_ref(
        es: &mut crate::model::EmptySurface,
        ctx: &mut CaptureCtx<'_>,
        surface_id: u32,
    ) -> Option<String> {
        let stored = es
            .deferred_spawn()
            .and_then(|s| s.scrollback_persist_id.clone());
        match stored {
            Some(existing) if !ctx.seen_refs.contains(&existing) => {
                ctx.seen_refs.insert(existing.clone());
                Some(existing)
            }
            Some(stale) => {
                // PTY가 없으므로 기존 파일 복사를 시도한다. 읽기·쓰기 실패에도 새 ID를 보관하는 동작은 동일하다.
                let new_id = crate::scrollback_store::new_persist_id();
                if let crate::scrollback_store::ScrollbackRead::Loaded(lines) =
                    crate::scrollback_store::read(&stale)
                {
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
                if let Some(spawn) = es.deferred_spawn_mut() {
                    spawn.scrollback_persist_id = Some(new_id.clone());
                }
                ctx.seen_refs.insert(new_id.clone());
                Some(new_id)
            }
            None => None,
        }
    }

    fn capture_generic_surface(surface: &dyn Surface, registry: &SurfaceKindRegistry) -> Self {
        let kind = surface.kind().to_string();
        if let Some(def) = registry.get(&kind) {
            // snapshot이 없어도 등록된 kind는 유지해 다음 복원에서 그 생성기를 사용할 수 있게 한다.
            let data = (def.snapshot)(surface).unwrap_or_else(|| json!({}));
            return SavedSurface::Generic { kind, data };
        }
        // 미등록 kind도 leaf 자리는 남겨 분할 구조를 유지한다.
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

    use crate::core::surface_registry::SurfaceKindRegistry;

    #[test]
    fn deferred_capture_ignores_surface_meta_restore_command() {
        let surface_id = 7u32;

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

        let waker: tasty_terminal::Waker = Arc::new(|| {});
        let spawn = crate::model::DeferredSpawn {
            shell: None,
            shell_args: Vec::new(),
            extra_env: Vec::new(),
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
                "deferred capture는 이전 ID의 surface 메타데이터를 복원 명령으로 쓰면 안 된다"
            ),
            _ => panic!("expected SavedSurface::Terminal"),
        }
    }

    /// 내용 대신 TerminalSurface marker만 넣어 workspace 필터·인덱스 처리를 확인한다.
    fn mirror_marker_ws(engine: &mut CoreState, name: &str, mirror: bool) -> Workspace {
        let ws_id = engine.next_ids.next_workspace();
        let pane_id = engine.next_ids.next_pane();
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let mut ws = Workspace::new_with_terminal_marker(
            ws_id,
            name.to_string(),
            pane_id,
            tab_id,
            surface_id,
        );
        ws.mirror = mirror;
        ws
    }

    /// scrollback 저장을 꺼 이 capture가 디스크를 쓰지 않게 한다. engine 생성의 파일 읽기까지 막지는 않는다.
    fn engine_with_workspaces(specs: &[(&str, bool)]) -> CoreState {
        let waker: tasty_terminal::Waker = Arc::new(|| {});
        let mut engine = CoreState::new(80, 24, waker).expect("engine");
        engine.settings.general.restore_surface_content = false;
        let workspaces: Vec<Workspace> = specs
            .iter()
            .map(|(name, mirror)| mirror_marker_ws(&mut engine, name, *mirror))
            .collect();
        engine.workspaces = workspaces;
        engine
    }

    #[test]
    fn capture_excludes_mirror_and_remaps_active() {
        let mut engine = engine_with_workspaces(&[
            ("n0", false),
            ("m1", true),
            ("n2", false),
            ("m3", true),
            ("n4", false),
        ]);
        let saved = SavedLayout::capture(&mut engine, 2);

        let names: Vec<&str> = saved.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["n0", "n2", "n4"],
            "mirror workspace 는 capture 에서 제외되어야 한다"
        );
        assert_eq!(
            saved.active_workspace, 1,
            "active_workspace 는 mirror 제외 후 인덱스로 remap 되어야 한다"
        );
    }

    #[test]
    fn capture_remaps_active_when_active_was_mirror() {
        let mut engine = engine_with_workspaces(&[("n0", false), ("m1", true), ("n2", false)]);
        let saved = SavedLayout::capture(&mut engine, 1);

        let names: Vec<&str> = saved.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["n0", "n2"]);
        assert_eq!(saved.active_workspace, 1);
    }

    #[test]
    fn capture_clamps_active_when_trailing_are_mirror() {
        let mut engine = engine_with_workspaces(&[("n0", false), ("m1", true)]);
        let saved = SavedLayout::capture(&mut engine, 1);

        let names: Vec<&str> = saved.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["n0"]);
        assert_eq!(
            saved.active_workspace, 0,
            "범위를 벗어난 active 는 마지막 유효 인덱스로 clamp 되어야 한다"
        );
    }
}
