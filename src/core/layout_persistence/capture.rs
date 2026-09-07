//! Live model (`CoreState`) → `SavedLayout` 캡처.
//!
//! `capture_scrollback_to_disk` 가 fresh persist_id 발급 + scrollback 디스크 dump 까지
//! 책임지므로 SeenRefs 추적으로 같은 capture 사이클의 중복 ID 를 self-heal.

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
        // mirror workspace(원격 attach 세션이 만든 로컬 미러)는 **비영속** 이다 — 원격 점유가
        // 살아있는 동안만 의미가 있고, layout.json 에 저장하면 재시작 시 원격 없는 **죽은 일반
        // workspace** 로 복원돼 버린다(N-RA02). capture 순회에서 제외한다. 제외로 인덱스가
        // 밀리므로 `active_workspace`(라이브 인덱스)도 필터 후 위치로 remap 한다.
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
        // remap 된 active 가 범위를 벗어나면(focus 가 mirror 였거나 뒤가 전부 mirror) 클램프.
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
        if let Some(ts) = surface
            .as_any()
            .downcast_ref::<crate::model::TerminalSurface>()
        {
            return Self::capture_terminal_surface(ts, ctx);
        }
        // 비활성 탭의 deferred EmptySurface 는 외부로는 Terminal 역할이지만
        // PTY 가 아직 안 떠 있어 TerminalSurface downcast 가 None 이다.
        // `DeferredSpawn` 자체가 들고 있는 cwd / restore_command / persist_id
        // 를 그대로 옮겨 round-trip 한다.
        if let Some(es) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::EmptySurface>()
        {
            // plugin placeholder(아직 실제화 안 된 non-terminal surface)는 원래
            // kind/snapshot 을 그대로 보존해 round-trip 한다 — 여기서 terminal 이나
            // "empty" 로 저장하면 다음 복원 때 plugin surface 가 딴것으로 변질된다.
            // `Deferred` 를 **enum 그대로** match 한다. 전에는 `deferred_plugin()` 을
            // 먼저 잡고 그다음 `is_deferred()` 로 갈랐는데, 그 정합은 **문장 순서**에만
            // 기대고 있었다 — 두 줄을 바꾸면 Plugin 이 아래 갈래로 떨어져 빈 필드의
            // `SavedSurface::Terminal` 로 **조용히 오변환**된다. variant 가 하나 늘 때도
            // 같은 일이 난다. match 면 컴파일러가 non-exhaustive 로 여기서 막는다.
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

    // deferred surface 복원 명령의 권위 출처는 `DeferredSpawn.restore_command`
    // (복원 시 layout 에서 이관됨) 뿐이다. `surface_meta[id]` 로 fallback 하면
    // 재사용된 id 에 남은 stale `claude -r …` 를 주워 담을 수 있으므로 읽지 않는다.
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
        // 옵션 on 일 때만 scrollback_ref 를 다음 capture 까지 유지한다.
        // (옵션 off 면 다음 capture 때 디스크 쓰기를 스킵하므로 ref 도 의미 없음 →
        //  파일은 startup GC 가 청소한다.)
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
                // PTY 가 안 떠 있어 직접 dump 불가 → 디스크 내용을 fresh
                // 파일로 복사하고 새 ID 를 발급. 한쪽이 라이브 surface 였다면
                // 그쪽은 이미 자기 데이터로 stale.bin 을 덮어쓴 후다.
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
            // snapshot 이 None 이어도(내용을 모름) kind 는 보존한다 — "내용을 모른다"
            // 는 "종류를 모른다" 가 아니다. kind 를 empty 로 버리면 재시작 시 그 자리가
            // 빈 empty 탭으로 살아나 종류마저 잃지만, 보존하면 registry 에 있는 kind 로
            // 복원되거나(hello 전이면) deferred plugin placeholder 로 살아난다. preset
            // capture(`preset_capture.rs`)가 같은 None 에 kind 를 보존하는 것과 정합 —
            // 같은 입력에 두 경로가 다르게 답하던 것(preset=kind 보존, layout=empty)을
            // 맞춘다.
            let data = (def.snapshot)(surface).unwrap_or_else(|| json!({}));
            return SavedSurface::Generic { kind, data };
        }
        // registry 에 아예 없는 kind(등록되지 않은 종류)만 Empty 로 fallback — leaf
        // 자체가 사라지면 split 구조가 어색해지므로.
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
                "deferred capture 는 surface_meta fallback 으로 stale 을 주워오면 안 된다"
            ),
            _ => panic!("expected SavedSurface::Terminal"),
        }
    }

    // ── N-RA02 회귀: mirror workspace 비영속 + active_workspace remap/clamp ──

    /// mirror workspace 하나를 담은 마커 워크스페이스를 만든다. capture 순회가
    /// 패닉 없이 통과하도록 TerminalSurface 마커를 채워둔다(내용은 검증 대상 아님).
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

    /// 주어진 (name, mirror) 목록으로 engine.workspaces 를 교체하고, scrollback
    /// 디스크 쓰기를 꺼(restore_surface_content=false) capture 를 순수 인메모리로 만든다.
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

    /// mirror workspace 는 SavedLayout.workspaces 에서 제외되고, active_workspace
    /// (활성이 일반 ws) 는 필터 후 인덱스로 remap 되어야 한다.
    #[test]
    fn capture_excludes_mirror_and_remaps_active() {
        // 라이브: [n0, m1(mirror), n2, m3(mirror), n4], active = 2 (n2).
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
        // n2 는 필터 후 인덱스 1 (앞의 non-mirror 는 n0 하나).
        assert_eq!(
            saved.active_workspace, 1,
            "active_workspace 는 mirror 제외 후 인덱스로 remap 되어야 한다"
        );
    }

    /// 활성 워크스페이스가 mirror 였다면, 그 자리는 붕괴하고 active 는 앞쪽
    /// non-mirror 개수(= 뒤따르는 non-mirror 의 인덱스)로 remap 된다.
    #[test]
    fn capture_remaps_active_when_active_was_mirror() {
        // 라이브: [n0, m1(mirror), n2], active = 1 (mirror m1).
        let mut engine = engine_with_workspaces(&[("n0", false), ("m1", true), ("n2", false)]);
        let saved = SavedLayout::capture(&mut engine, 1);

        let names: Vec<&str> = saved.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["n0", "n2"]);
        // m1 앞의 non-mirror 는 n0 하나 → active 는 1 (= n2 의 saved 인덱스).
        assert_eq!(saved.active_workspace, 1);
    }

    /// remap 된 active 가 필터 결과 범위를 벗어나면(활성이 마지막 mirror 라 뒤에
    /// non-mirror 가 없음) 마지막 유효 인덱스로 clamp 되어야 한다.
    #[test]
    fn capture_clamps_active_when_trailing_are_mirror() {
        // 라이브: [n0, m1(mirror)], active = 1 (마지막 mirror).
        let mut engine = engine_with_workspaces(&[("n0", false), ("m1", true)]);
        let saved = SavedLayout::capture(&mut engine, 1);

        let names: Vec<&str> = saved.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["n0"]);
        // 앞의 non-mirror 1 개 → remap=1 이지만 saved.len()=1 이므로 0 으로 clamp.
        assert_eq!(
            saved.active_workspace, 0,
            "범위를 벗어난 active 는 마지막 유효 인덱스로 clamp 되어야 한다"
        );
    }
}
