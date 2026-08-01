use super::FocusDirection;
use super::surface_layout::SurfaceLayout;
use super::surface_trait::Surface;
use super::{SplitDirection, SurfaceId, TabId, TerminalSurface};
use tasty_terminal::Terminal;

pub struct Tab {
    pub id: TabId,
    /// Auto-generated name (e.g. "Shell"). Used as fallback when explicit_name is None.
    pub name: String,
    /// Explicitly set tab name. When Some, overrides everything else.
    pub explicit_name: Option<String>,
    /// OSC 0/2 로 받은 terminal window title. `explicit_name` 보다 낮고
    /// `cached_display_name` 보다 높은 우선순위. cwd 변경 시 shell prompt 가
    /// 새 OSC title 을 자연 발화하므로 cwd 도 자연 반영된다. layout.json
    /// 영속 대상 아님 — runtime only.
    pub osc_title: Option<String>,
    /// The layout tree of surfaces. Always a binary tree; a single leaf = unsplit state.
    /// Temporarily `None` during structural mutations (take_layout/put_layout pattern).
    /// Deferred terminals live inside the layout as `EmptySurface { deferred_spawn: Some(..) }`
    /// placeholders, NOT as a None layout.
    pub layout_opt: Option<SurfaceLayout>,
    /// The focused surface ID within this tab's layout.
    pub focused_surface: SurfaceId,
    /// Cached display name. Updated on CwdChanged/explicit_name change, not every frame.
    pub cached_display_name: Option<String>,
}

impl Tab {
    /// Create a tab with a Surface trait object.
    pub fn new_with_surface(id: TabId, name: String, surface: Box<dyn Surface>) -> Self {
        Self::new_named(id, name, None, surface)
    }

    /// `new_with_surface` 의 명시 이름 버전. `explicit_name` 이 `Some` 이면 **생성
    /// 시점부터** 그 이름이 고정되어 `display_name()` 에서 최우선으로 쓰인다
    /// (cwd / OSC title 로 덮이지 않음). 에이전트가 `tab.create --name` 으로 탭을
    /// 만들 때 사용한다.
    pub fn new_named(
        id: TabId,
        name: String,
        explicit_name: Option<String>,
        surface: Box<dyn Surface>,
    ) -> Self {
        let surface_id = surface.surface_id().unwrap_or(0);
        Self {
            id,
            name,
            explicit_name,
            osc_title: None,
            layout_opt: Some(SurfaceLayout::Leaf(surface)),
            focused_surface: surface_id,
            cached_display_name: None,
        }
    }

    /// Get the display name for this tab (cached, no syscalls).
    /// Priority: explicit_name > osc_title > cached CWD-derived name > fallback "name" field.
    pub fn display_name(&self) -> String {
        if let Some(ref explicit) = self.explicit_name {
            return explicit.clone();
        }
        if let Some(ref osc) = self.osc_title {
            return osc.clone();
        }
        if let Some(ref cached) = self.cached_display_name {
            return cached.clone();
        }
        self.name.clone()
    }

    /// Recompute and cache the display name from the focused terminal's CWD.
    /// Caller (CoreState::refresh_tab_display_name) lookups Terminal via
    /// `engine.terminals.get(focused_surface).and_then(|t| t.get_cwd())` first
    /// and passes the cwd in. Tab itself doesn't see the TerminalStore.
    pub fn refresh_display_name(&mut self, cwd: Option<&std::path::Path>) {
        if self.explicit_name.is_some() {
            return;
        }
        if let Some(cwd) = cwd {
            if let Some(home) = dirs_home()
                && cwd == home
            {
                self.cached_display_name = Some("~".to_string());
                return;
            }
            let path_str = cwd.to_string_lossy();
            if path_str == "/" {
                self.cached_display_name = Some("/".to_string());
                return;
            }
            if let Some(name) = cwd.file_name() {
                self.cached_display_name = Some(name.to_string_lossy().to_string());
                return;
            }
        }
        self.cached_display_name = None;
    }

    // ── Layout-based accessors ──

    /// Access the layout.
    #[track_caller]
    pub fn layout(&self) -> &SurfaceLayout {
        self.layout_opt
            .as_ref()
            .expect("BUG: no layout (deferred tab not initialized?)")
    }

    /// Access the layout mutably.
    #[track_caller]
    pub fn layout_mut(&mut self) -> &mut SurfaceLayout {
        self.layout_opt
            .as_mut()
            .expect("BUG: no layout (deferred tab not initialized?)")
    }

    /// Access the layout if initialized.
    pub fn layout_if_initialized(&self) -> Option<&SurfaceLayout> {
        self.layout_opt.as_ref()
    }

    /// Take the layout out (for structural mutation). Must be followed by put_layout.
    #[track_caller]
    pub fn take_layout(&mut self) -> SurfaceLayout {
        self.layout_opt.take().expect("BUG: layout already taken")
    }

    /// Put the layout back after structural mutation.
    pub fn put_layout(&mut self, layout: SurfaceLayout) {
        self.layout_opt = Some(layout);
    }

    // ── Surface delegation (backward-compat helpers) ──

    /// Get the "surface" for this tab. For a single leaf, returns the leaf.
    /// For a split, returns the focused leaf surface.
    /// NOTE: Callers that need the full layout tree should use layout() instead.
    #[track_caller]
    pub fn surface(&self) -> &dyn Surface {
        let layout = self.layout();
        // For a single leaf, return it directly
        if let SurfaceLayout::Leaf(surface) = layout {
            return surface.as_ref();
        }
        // For splits, return the focused leaf
        if let Some(leaf) = layout.find_surface(self.focused_surface) {
            return leaf;
        }
        // Fallback: first leaf
        if let Some(first_id) = layout.first_surface_id()
            && let Some(leaf) = layout.find_surface(first_id)
        {
            return leaf;
        }
        panic!("BUG: layout has no surfaces");
    }

    /// Get the focused surface mutably.
    /// NOTE: Callers that need the full layout tree should use layout_mut() instead.
    #[track_caller]
    pub fn surface_mut(&mut self) -> &mut dyn Surface {
        let focused = self.focused_surface;
        let layout = self.layout_mut();
        // For a single leaf, return it directly
        if let SurfaceLayout::Leaf(surface) = layout {
            return surface.as_mut();
        }
        // Determine which ID to look up
        let target_id = if layout.contains_surface(focused) {
            focused
        } else {
            layout
                .first_surface_id()
                .expect("BUG: layout has no surfaces")
        };
        layout
            .find_leaf_mut(target_id)
            .map(|b| b.as_mut())
            .expect("BUG: layout has no surfaces")
    }

    /// Access the surface if initialized (for backward compat).
    pub fn surface_if_initialized(&self) -> Option<&dyn Surface> {
        let layout = self.layout_opt.as_ref()?;
        if let SurfaceLayout::Leaf(surface) = layout {
            return Some(surface.as_ref());
        }
        if let Some(leaf) = layout.find_surface(self.focused_surface) {
            return Some(leaf);
        }
        layout
            .first_surface_id()
            .and_then(|id| layout.find_surface(id))
    }

    /// Whether the layout is a split (more than one surface).
    pub fn is_split(&self) -> bool {
        matches!(self.layout_opt.as_ref(), Some(SurfaceLayout::Split { .. }))
    }

    /// All surface IDs in this tab.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.layout().all_surface_ids()
    }

    /// Whether this tab contains the given surface ID.
    pub fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        match &self.layout_opt {
            Some(layout) => layout.contains_surface(surface_id),
            None => false,
        }
    }

    /// Get the focused surface ID.
    pub fn focused_surface_id(&self) -> Option<SurfaceId> {
        Some(self.focused_surface)
    }

    /// Visit every leaf Surface (read-only). 닫기 경로의 persist_id 수집용.
    pub fn for_each_surface(&self, f: &mut dyn FnMut(&dyn crate::Surface)) {
        if let Some(layout) = self.layout_opt.as_ref() {
            layout.for_each_surface(f);
        }
    }

    // ── Surface layout operations ──

    /// Close a surface within this tab. Returns true if found and closed.
    pub fn close_surface(&mut self, target_id: SurfaceId) -> bool {
        let old_layout = self.take_layout();
        let (new_layout, found) = old_layout.close_surface(target_id);
        self.put_layout(new_layout);
        if found
            && self.focused_surface == target_id
            && let Some(first_id) = self.layout().first_surface_id()
        {
            self.focused_surface = first_id;
        }
        found
    }

    /// Move focus to the next surface.
    pub fn move_focus_forward(&mut self) {
        let ids = self.layout().all_surface_ids();
        if ids.len() <= 1 {
            return;
        }
        let pos = ids
            .iter()
            .position(|&id| id == self.focused_surface)
            .unwrap_or(0);
        self.focused_surface = ids[(pos + 1) % ids.len()];
    }

    /// Move focus to the previous surface.
    pub fn move_focus_backward(&mut self) {
        let ids = self.layout().all_surface_ids();
        if ids.len() <= 1 {
            return;
        }
        let pos = ids
            .iter()
            .position(|&id| id == self.focused_surface)
            .unwrap_or(0);
        self.focused_surface = ids[(pos + ids.len() - 1) % ids.len()];
    }

    /// Directional focus navigation.
    pub fn directional_focus(&self, direction: FocusDirection) -> Option<SurfaceId> {
        self.layout()
            .directional_focus(self.focused_surface, direction)
    }

    /// Resize all surfaces within the layout.
    pub fn resize_all(&mut self, rect: super::PhysicalRect, cell_width: f32, cell_height: f32) {
        if let Some(layout) = self.layout_opt.as_mut() {
            layout.resize_all(rect, cell_width, cell_height);
        }
    }

    /// All surface regions with Surface trait references.
    pub fn surface_regions(&self, rect: super::PhysicalRect) -> Vec<super::SurfaceRegion<'_>> {
        self.layout().surface_regions(rect)
    }

    // ── Initialization ──

    /// deferred PTY spawn 을 영구 실패로 판단하기까지 허용하는 연속 시도 횟수.
    /// transient 실패는 보통 1~2 프레임 내 성공하므로 그 전에 치유되고, 이 상한에
    /// 도달하면 reify 의 매 프레임 재시도 폭주를 멈춘다.
    const MAX_SPAWN_ATTEMPTS: u32 = 5;

    /// 특정 surface_id에 해당하는 deferred placeholder를 찾아 PTY를 spawn하고
    /// `TerminalSurface` marker로 교체. spawn된 경우 `Some((terminal, persist_id))`
    /// 를 반환 — caller가 `engine.terminals.insert` + `set_scrollback_persist_id`
    /// 로 store 에 넣는다. None 이면 spawn 실패 또는 deferred 아님.
    pub fn ensure_initialized(
        &mut self,
        surface_id: SurfaceId,
    ) -> Option<(Terminal, Option<String>)> {
        let layout = self.layout_opt.as_mut()?;
        let leaf = layout.find_leaf_mut(surface_id)?;
        let empty = leaf.as_any_mut().downcast_mut::<super::EmptySurface>()?;
        // 영구 실패로 판단된 placeholder 는 더 이상 재시도하지 않는다. reify 는 매
        // 프레임(~60fps) 호출되므로 이 가드가 없으면 초당 수십 회 spawn + 로그 플러드.
        if empty.spawn_attempts >= Self::MAX_SPAWN_ATTEMPTS {
            return None;
        }
        // spawn 정보를 take 하지 않고 clone 한다. PTY spawn 이 실패해도
        // placeholder 의 deferred_spawn 이 남아 있어야 다음 reify 트리거에서
        // 재시도된다. (waker 는 Arc, 나머지는 작은 문자열/벡터라 clone 이 저렴.)
        let spawn = empty.deferred_spawn.clone()?;
        let persist_id = spawn.scrollback_persist_id.clone();
        let terminal = match spawn_terminal_from_deferred(surface_id, spawn) {
            Ok(t) => t,
            Err(e) => {
                // 실패: leaf 는 여전히 deferred EmptySurface 라 재시도 경로가 살아 있다.
                // 실패 횟수를 누적해 상한에서 폭주를 멈춘다. transient 실패는 상한 전에
                // 성공해 자가 치유된다.
                empty.spawn_attempts += 1;
                if empty.spawn_attempts >= Self::MAX_SPAWN_ATTEMPTS {
                    tracing::error!(
                        "surface {surface_id}: PTY spawn {}회 연속 실패 — 재시도 중단: {e}",
                        Self::MAX_SPAWN_ATTEMPTS
                    );
                } else if empty.spawn_attempts == 1 {
                    tracing::warn!("surface {surface_id}: PTY spawn 실패 (재시도 예정): {e}");
                }
                return None;
            }
        };
        // spawn 성공: 이제 placeholder 를 TerminalSurface marker 로 교체한다.
        // (EmptySurface 전체가 drop 되므로 deferred_spawn 도 함께 사라진다.)
        let ts: Box<dyn Surface> = Box::new(TerminalSurface { id: surface_id });
        *leaf = ts;
        Some((terminal, persist_id))
    }

    /// 이 탭의 layout 안에 deferred placeholder로 남아있는 모든 surface ID를 spawn.
    /// 반환값은 `(surface_id, Terminal, persist_id)` 의 목록 — caller 가 store 에
    /// insert 한다.
    pub fn ensure_all_initialized(&mut self) -> Vec<(SurfaceId, Terminal, Option<String>)> {
        let ids = self.deferred_surface_ids();
        let mut spawned = Vec::with_capacity(ids.len());
        for sid in ids {
            if let Some((t, pid)) = self.ensure_initialized(sid) {
                spawned.push((sid, t, pid));
            }
        }
        spawned
    }

    /// layout 안에 deferred EmptySurface placeholder가 하나라도 있으면 true.
    pub fn is_deferred(&self) -> bool {
        !self.deferred_surface_ids().is_empty()
    }

    /// layout 안의 모든 deferred EmptySurface placeholder의 surface_id 목록.
    pub fn deferred_surface_ids(&self) -> Vec<SurfaceId> {
        let mut out = Vec::new();
        if let Some(layout) = self.layout_opt.as_ref() {
            collect_deferred_ids(layout, &mut out);
        }
        out
    }

    /// 주어진 surface_id가 이 탭의 deferred placeholder인지 확인.
    pub fn is_surface_deferred(&self, surface_id: SurfaceId) -> bool {
        let Some(layout) = self.layout_opt.as_ref() else {
            return false;
        };
        let Some(leaf) = layout.find_surface(surface_id) else {
            return false;
        };
        leaf.as_any()
            .downcast_ref::<super::EmptySurface>()
            .map(|e| e.is_deferred())
            .unwrap_or(false)
    }

    /// Replace the entire layout with a single surface.
    pub fn put_surface(&mut self, surface: Box<dyn Surface>) {
        let sid = surface.surface_id().unwrap_or(0);
        self.layout_opt = Some(SurfaceLayout::Leaf(surface));
        self.focused_surface = sid;
    }

    // ── Split operations ──

    /// Split the focused surface within this tab with a TerminalSurface marker.
    /// Moves focus to the new surface. Caller must have already inserted the
    /// spawned Terminal into `CoreState::terminals`.
    pub fn split_focused_surface(&mut self, direction: SplitDirection, new_surface_id: SurfaceId) {
        let new_node = TerminalSurface { id: new_surface_id };
        let target = self.focused_surface;
        let old_layout = self.take_layout();
        let (new_layout, _) = old_layout.split_with_node(target, direction, new_node);
        self.put_layout(new_layout);
        self.focused_surface = new_surface_id;
    }

    /// Split a specific surface by ID with a TerminalSurface marker. Does NOT
    /// change focused_surface. Caller must have already inserted the spawned
    /// Terminal into `CoreState::terminals`.
    pub fn split_surface_by_id(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
    ) -> bool {
        let new_node = TerminalSurface { id: new_surface_id };
        let old_layout = self.take_layout();
        let (new_layout, remaining) =
            old_layout.split_with_node(target_surface_id, direction, new_node);
        self.put_layout(new_layout);
        remaining.is_none()
    }

    /// Split a specific surface by ID with any surface type.
    pub fn split_surface_by_id_generic(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface: Box<dyn Surface>,
    ) -> bool {
        let old_layout = self.take_layout();
        let (new_layout, remaining) =
            old_layout.split_with_surface(target_surface_id, direction, new_surface);
        self.put_layout(new_layout);
        if remaining.is_some() {
            tracing::warn!(
                "split_surface_by_id_generic: target {} not found",
                target_surface_id
            );
        }
        remaining.is_none()
    }

    /// Produce a JSON tree representation of this tab.
    pub fn to_tree_json(&self) -> serde_json::Value {
        let layout_json = if self.is_split() {
            let mut v = serde_json::json!({
                "type": "SplitLayout",
                "focused_surface": self.focused_surface,
                "surfaces": self.all_surface_ids(),
            });
            // 분할 방향/비율/상위-하위 중첩 구조를 보존한 전체 트리. CLI `list tree`
            // 가 이걸로 split tab 의 SurfaceGroup 계층을 그린다. flat `surfaces` 는
            // 호환을 위해 남겨둔다.
            if let Some(layout) = self.layout_if_initialized() {
                v["layout"] = layout.to_tree_json_full();
            }
            v
        } else {
            // Single-leaf tab. EmptySurface(deferred) renders itself with pty_ready: false.
            // For a live TerminalSurface, append pty_ready: true.
            let mut v = self.surface().to_tree_json();
            if v.get("type").and_then(|t| t.as_str()) == Some("Terminal")
                && !v
                    .as_object()
                    .map(|o| o.contains_key("pty_ready"))
                    .unwrap_or(false)
                && let Some(obj) = v.as_object_mut()
            {
                obj.insert("pty_ready".into(), serde_json::json!(true));
            }
            v
        };
        serde_json::json!({
            "id": self.id,
            "name": self.display_name(),
            "surface": layout_json,
        })
    }
}

fn collect_deferred_ids(layout: &SurfaceLayout, out: &mut Vec<SurfaceId>) {
    match layout {
        SurfaceLayout::Leaf(surface) => {
            if let Some(empty) = surface.as_any().downcast_ref::<super::EmptySurface>()
                && empty.is_deferred()
            {
                out.push(empty.id);
            }
        }
        SurfaceLayout::Split { first, second, .. } => {
            collect_deferred_ids(first, out);
            collect_deferred_ids(second, out);
        }
    }
}

fn spawn_terminal_from_deferred(
    surface_id: SurfaceId,
    spawn: super::terminal_surface::DeferredSpawn,
) -> Result<Terminal, String> {
    let shell_ref = spawn.shell.as_deref();
    let shell_args: Vec<&str> = spawn.shell_args.iter().map(|s| s.as_str()).collect();
    let extra_env: Vec<(&str, &str)> = spawn
        .extra_env
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let working_dir = spawn.working_dir.as_deref();
    // PTY master 의 첫 입력으로 restore_command 를 미리 적재. Terminal::new 가
    // writer thread spawn 전에 동기 write 하므로, shell 이 stdin 을 처음 read
    // 하는 순간 이 명령이 들어간다 (예: `claude -r <uuid>\r`).
    let initial = spawn.restore_command.as_deref().map(|c| format!("{c}\r"));
    let initial_input = initial.as_deref();
    match Terminal::new(
        tasty_terminal::TerminalConfig {
            cols: spawn.cols,
            rows: spawn.rows,
            shell: shell_ref,
            args: &shell_args,
            surface_id,
            working_dir,
            initial_input,
            extra_env: &extra_env,
        },
        spawn.waker,
    ) {
        Ok(terminal) => Ok(terminal),
        Err(e) => Err(e.to_string()),
    }
}

fn dirs_home() -> Option<std::path::PathBuf> {
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().map(std::path::PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(std::path::PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EmptySurface;
    use crate::terminal_surface::DeferredSpawn;
    use std::sync::Arc;

    fn deferred_spawn(shell: Option<&str>) -> DeferredSpawn {
        DeferredSpawn {
            shell: shell.map(|s| s.to_string()),
            shell_args: Vec::new(),
            extra_env: Vec::new(),
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}) as tasty_terminal::Waker,
            working_dir: None,
            restore_command: None,
            scrollback_persist_id: None,
        }
    }

    fn deferred_tab(sid: SurfaceId, spawn: DeferredSpawn) -> Tab {
        let surface: Box<dyn Surface> = Box::new(EmptySurface::new_deferred(sid, spawn));
        Tab::new_with_surface(1, "t".to_string(), surface)
    }

    /// 핵심 회귀: PTY spawn 이 실패해도 placeholder 의 deferred_spawn 이 보존되어
    /// surface 가 deferred 로 남고 다음 reify 트리거에서 재시도 가능해야 한다.
    /// (수정 전: take-before-spawn 이라 실패 시 정보가 소실되어 영구 빈 surface.)
    #[test]
    fn ensure_initialized_failure_keeps_surface_deferred() {
        let sid: SurfaceId = 42;
        // 존재하지 않는 shell → Terminal::new 의 spawn_command 가 실패한다.
        let mut tab = deferred_tab(
            sid,
            deferred_spawn(Some("/nonexistent/tasty_no_such_shell")),
        );
        assert!(tab.is_surface_deferred(sid));

        let result = tab.ensure_initialized(sid);
        assert!(result.is_none(), "spawn 실패 시 None");
        assert!(
            tab.is_surface_deferred(sid),
            "spawn 실패 후에도 deferred 유지되어 재시도 가능해야 함 (수정 전엔 stranded)"
        );

        // 정보가 보존되었으므로 두 번째 reify 호출도 동일하게 재시도 가능.
        assert!(tab.ensure_initialized(sid).is_none());
        assert!(tab.is_surface_deferred(sid));
    }

    /// 테스트 헬퍼: layout 안 EmptySurface 의 spawn_attempts 를 읽는다.
    fn spawn_attempts_of(tab: &Tab, sid: SurfaceId) -> Option<u32> {
        tab.layout_opt
            .as_ref()?
            .find_surface(sid)?
            .as_any()
            .downcast_ref::<EmptySurface>()
            .map(|e| e.spawn_attempts)
    }

    /// 영구 실패 케이스: 연속 실패가 MAX_SPAWN_ATTEMPTS 에서 capped 되어 더 이상
    /// spawn 을 재시도하지 않는다 (reify 매 프레임 폭주 차단). placeholder 는 남는다.
    #[test]
    fn ensure_initialized_stops_after_max_attempts() {
        let sid: SurfaceId = 99;
        let mut tab = deferred_tab(
            sid,
            deferred_spawn(Some("/nonexistent/tasty_no_such_shell")),
        );
        assert!(tab.is_surface_deferred(sid));

        // 상한보다 넉넉히 더 호출해도 매번 None.
        for _ in 0..(Tab::MAX_SPAWN_ATTEMPTS + 3) {
            assert!(
                tab.ensure_initialized(sid).is_none(),
                "영구 실패는 항상 None"
            );
        }

        // 실패 카운터가 상한에서 capped — 더 늘지 않아 폭주가 멈췄다는 증거.
        assert_eq!(
            spawn_attempts_of(&tab, sid),
            Some(Tab::MAX_SPAWN_ATTEMPTS),
            "spawn_attempts 가 MAX 에서 capped 되어야 함"
        );
        // placeholder 는 여전히 남는다 (TerminalSurface 로 교체되지 않음).
        assert!(tab.is_surface_deferred(sid));
    }

    /// 성공 경로 회귀 고정: spawn 이 성공하면 leaf 가 TerminalSurface 로 교체되고
    /// deferred 가 해제된다.
    #[test]
    fn ensure_initialized_success_replaces_leaf() {
        let sid: SurfaceId = 7;
        // shell None → default_shell 로 실제 PTY spawn (테스트 머신에 shell 존재).
        let mut tab = deferred_tab(sid, deferred_spawn(None));
        assert!(tab.is_surface_deferred(sid));

        let result = tab.ensure_initialized(sid);
        assert!(result.is_some(), "정상 shell 은 spawn 성공");
        assert!(
            !tab.is_surface_deferred(sid),
            "성공 시 deferred 해제 + TerminalSurface 로 교체"
        );
    }
}
