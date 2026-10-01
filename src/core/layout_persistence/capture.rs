//! Legacy runtime capture for import/shadow comparison fixtures.
//! Product export resolves a fixed journal model through journal_payload::legacy_export.

use serde_json::json;

use crate::model::{Deferred, Pane, PaneNode, Surface, SurfaceLayout, Tab, Workspace};
use crate::runtime::engine_access::EngineMut;
use crate::runtime::surface_registry::SurfaceKindRegistry;

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
    presentation: &'a dyn crate::model::StructurePresentation,
    registry: &'a SurfaceKindRegistry,
    capture_scrollback: bool,
    memory: &'a MemArc,
    seen_refs: &'a mut SeenRefs,
    terminals: &'a mut crate::runtime::terminal_store::TerminalStore,
}

impl SavedLayout {
    /// Capture runtime instances before walking descriptors. Missing owners abort the export.
    pub fn capture(
        engine: &mut EngineMut<'_>,
        active_workspace: usize,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> Result<Self, String> {
        let registry = engine.runtime.surface_registry.clone();
        let memory = engine.runtime.memory.clone();
        let capture_scrollback = engine.runtime.settings.general.restore_surface_content;
        let mut seen_refs = SeenRefs::new();
        let active_workspace = engine
            .workspaces()
            .iter()
            .take(active_workspace)
            .filter(|workspace| !workspace.mirror)
            .count();
        let ids: Vec<_> = engine
            .core
            .local_workspaces()
            .iter()
            .flat_map(|workspace| workspace.all_surface_ids())
            .collect();
        let mut captured = std::collections::HashMap::new();
        let mut ctx = CaptureCtx {
            presentation,
            registry: &registry,
            capture_scrollback,
            memory: &memory,
            seen_refs: &mut seen_refs,
            terminals: &mut engine.runtime.terminals,
        };
        for id in ids {
            let instance = engine
                .runtime
                .surfaces
                .get_mut(&id)
                .ok_or_else(|| format!("surface {id} has no runtime owner during capture"))?;
            let snapshot = SavedSurface::capture_surface(instance.as_mut(), &mut ctx);
            if captured.insert(id, snapshot).is_some() {
                return Err(format!(
                    "surface {id} appears twice in the committed structure"
                ));
            }
        }
        let workspaces = engine
            .core
            .local_workspaces()
            .iter()
            .map(|workspace| {
                let focused = workspace
                    .pane_layout()
                    .all_pane_ids()
                    .iter()
                    .position(|id| Some(*id) == presentation.pane_id(workspace))
                    .unwrap_or(0);
                Ok(SavedWorkspace {
                    name: workspace.name.clone(),
                    subtitle: workspace.subtitle.clone(),
                    description: workspace.description.clone(),
                    pane_layout: SavedPaneNode::capture(
                        workspace.pane_layout(),
                        presentation,
                        &mut captured,
                    )?,
                    focused_pane_index: focused,
                    attach_mapping: workspace.attach_mapping.clone(),
                    category: workspace.category,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let active_workspace = active_workspace.min(workspaces.len().saturating_sub(1));
        Ok(Self {
            version: LAYOUT_VERSION,
            workspaces,
            active_workspace,
            categories: engine
                .categories()
                .iter()
                .map(|category| SavedCategory {
                    id: category.id,
                    name: category.name.clone(),
                    collapsed: presentation.category_collapsed(category.id),
                })
                .collect(),
        })
    }
}
impl SavedPaneNode {
    fn capture(
        node: &PaneNode,
        presentation: &dyn crate::model::StructurePresentation,
        captured: &mut std::collections::HashMap<u32, SavedSurface>,
    ) -> Result<Self, String> {
        Ok(match node {
            PaneNode::Leaf(pane) => Self::Leaf(SavedPane {
                active_tab: presentation.tab_index(pane),
                tabs: pane
                    .tabs
                    .iter()
                    .map(|tab| {
                        Ok(SavedTab {
                            name: tab.name.clone(),
                            explicit_name: tab.explicit_name.clone(),
                            surface: SavedSurfaceLayout::capture_layout(tab.layout(), captured)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            }),
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => Self::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(Self::capture(first, presentation, captured)?),
                second: Box::new(Self::capture(second, presentation, captured)?),
            },
        })
    }
}
impl SavedSurfaceLayout {
    fn capture_layout(
        layout: &SurfaceLayout,
        captured: &mut std::collections::HashMap<u32, SavedSurface>,
    ) -> Result<Self, String> {
        Ok(match layout {
            SurfaceLayout::Leaf(surface) => Self::Leaf(
                captured
                    .remove(&surface.id)
                    .ok_or_else(|| format!("surface {} capture is missing", surface.id))?,
            ),
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => Self::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(Self::capture_layout(first, captured)?),
                second: Box::new(Self::capture_layout(second, captured)?),
            },
        })
    }
}

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
            .cwd(surface_id)
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

    use crate::runtime::surface_registry::SurfaceKindRegistry;

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

        let spawn = crate::model::DeferredSpawn {
            shell: None,
            shell_args: Vec::new(),
            extra_env: Vec::new(),
            cols: 80,
            rows: 24,
            working_dir: None,
            restore_command: None,
            scrollback_persist_id: None,
        };
        let mut es = crate::model::EmptySurface::new_deferred(surface_id, spawn);

        let registry = SurfaceKindRegistry::new();
        let mut seen_refs = SeenRefs::new();
        let mut terminals =
            crate::runtime::terminal_store::TerminalStore::new(std::sync::Arc::new(
                std::sync::atomic::AtomicU32::new(crate::runtime::terminal_store::PTY_ID_BASE),
            ));
        let mut ctx = CaptureCtx {
            presentation: &crate::model::StructurePresentationSnapshot::default(),
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
    fn mirror_marker_ws(engine: &mut EngineMut<'_>, name: &str, mirror: bool) -> Workspace {
        let ws_id = engine.runtime.counters.next_workspace();
        let pane_id = engine.runtime.counters.next_pane();
        let tab_id = engine.runtime.counters.next_tab();
        let surface_id = engine.runtime.counters.next_surface();
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
    fn engine_with_workspaces(
        specs: &[(&str, bool)],
    ) -> crate::runtime::engine_session::EngineSession {
        let waker: tasty_terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine");
        let mut engine = engine_session.borrow_mut();
        engine.runtime.settings.general.restore_surface_content = false;
        let workspaces: Vec<Workspace> = specs
            .iter()
            .map(|(name, mirror)| mirror_marker_ws(&mut engine, name, *mirror))
            .collect();
        engine.set_workspace_fixture(workspaces);
        engine_session
    }

    #[test]
    fn capture_excludes_mirror_and_remaps_active() {
        let mut engine_session = engine_with_workspaces(&[
            ("n0", false),
            ("m1", true),
            ("n2", false),
            ("m3", true),
            ("n4", false),
        ]);
        let mut engine = engine_session.borrow_mut();
        let saved = SavedLayout::capture(
            &mut engine,
            2,
            &crate::model::StructurePresentationSnapshot::default(),
        );

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
        let mut engine_session =
            engine_with_workspaces(&[("n0", false), ("m1", true), ("n2", false)]);
        let mut engine = engine_session.borrow_mut();
        let saved = SavedLayout::capture(
            &mut engine,
            1,
            &crate::model::StructurePresentationSnapshot::default(),
        );

        let names: Vec<&str> = saved.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["n0", "n2"]);
        assert_eq!(saved.active_workspace, 1);
    }

    #[test]
    fn capture_clamps_active_when_trailing_are_mirror() {
        let mut engine_session = engine_with_workspaces(&[("n0", false), ("m1", true)]);
        let mut engine = engine_session.borrow_mut();
        let saved = SavedLayout::capture(
            &mut engine,
            1,
            &crate::model::StructurePresentationSnapshot::default(),
        );

        let names: Vec<&str> = saved.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["n0"]);
        assert_eq!(
            saved.active_workspace, 0,
            "범위를 벗어난 active 는 마지막 유효 인덱스로 clamp 되어야 한다"
        );
    }
}
