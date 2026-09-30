use super::surface_layout::SurfaceLayout;
use super::surface_trait::Surface;
use super::terminal_surface::DeferredSpawn;
use super::{SplitDirection, SurfaceId, TabId, TerminalSurface};

#[derive(Default)]
pub struct SurfaceTitle {
    pub osc_title: Option<String>,
    pub cwd_name: Option<String>,
}

pub struct Tab {
    pub id: TabId,
    /// Auto-generated name (e.g. "Shell"). Used as fallback when explicit_name is None.
    pub name: String,
    /// Explicitly set tab name. When Some, overrides everything else.
    pub explicit_name: Option<String>,
    /// surface의 이진트리. take_layout/put_layout 사이에만 None이다.
    /// 지연 생성은 트리 안 EmptySurface의 Deferred 값으로 표현한다.
    pub layout_opt: Option<SurfaceLayout>,
    /// Per-surface observed titles. Selection is supplied by the presentation caller.
    pub surface_titles: std::collections::HashMap<SurfaceId, SurfaceTitle>,
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
        Self {
            id,
            name,
            explicit_name,
            layout_opt: Some(SurfaceLayout::Leaf(surface)),
            surface_titles: Default::default(),
        }
    }

    /// Get the display name for this tab (cached, no syscalls).
    /// Priority: explicit_name > osc_title > cached CWD-derived name > fallback "name" field.
    pub fn display_name(&self, surface_id: Option<SurfaceId>) -> String {
        if let Some(ref explicit) = self.explicit_name {
            return explicit.clone();
        }
        if let Some(title) = surface_id.and_then(|id| self.surface_titles.get(&id)) {
            if let Some(osc) = &title.osc_title {
                return osc.clone();
            }
            if let Some(cwd) = &title.cwd_name {
                return cwd.clone();
            }
        }
        self.name.clone()
    }

    /// Recompute and cache the display name from the focused terminal's CWD.
    /// Caller (CoreState::refresh_tab_display_name) lookups Terminal via
    /// `engine.runtime.terminals.get(focused_surface).and_then(|t| t.get_cwd())` first
    /// and passes the cwd in. Tab itself doesn't see the TerminalStore.
    pub fn refresh_display_name(&mut self, surface_id: SurfaceId, cwd: Option<&std::path::Path>) {
        let title = self.surface_titles.entry(surface_id).or_default();
        if let Some(cwd) = cwd {
            if let Some(home) = dirs_home()
                && cwd == home
            {
                title.cwd_name = Some("~".to_string());
                return;
            }
            let path_str = cwd.to_string_lossy();
            if path_str == "/" {
                title.cwd_name = Some("/".to_string());
                return;
            }
            if let Some(name) = cwd.file_name() {
                title.cwd_name = Some(name.to_string_lossy().to_string());
                return;
            }
        }
        title.cwd_name = None;
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
        let ids = layout.all_surface_ids();
        self.surface_titles.retain(|id, _| ids.contains(id));
        self.layout_opt = Some(layout);
    }

    /// Find exactly the requested surface; a stale ID never selects a sibling.
    pub fn surface(&self, surface_id: SurfaceId) -> Option<&dyn Surface> {
        self.layout_opt.as_ref()?.find_surface(surface_id)
    }

    pub fn surface_mut(&mut self, surface_id: SurfaceId) -> Option<&mut (dyn Surface + 'static)> {
        match self.layout_opt.as_mut()?.find_leaf_mut(surface_id) {
            Some(surface) => Some(surface.as_mut()),
            None => None,
        }
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

    /// A deterministic structural representative, independent from navigation.
    pub fn first_surface_id(&self) -> Option<SurfaceId> {
        self.layout_if_initialized()?.first_surface_id()
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
        found
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

    /// PTY 생성의 연속 실패 뒤 재시도를 멈출 횟수.
    pub const MAX_SPAWN_ATTEMPTS: u32 = 5;

    /// 터미널 placeholder의 PTY 생성 정보를 복사해 반환한다. model은 PTY를 만들지 않는다.
    /// 호스트가 이 정보로 PTY를 만든 뒤 성공하면 [`Tab::complete_terminal_spawn`],
    /// 실패하면 [`Tab::record_terminal_spawn_failure`]를 호출한다.
    /// 터미널 placeholder가 아니거나 연속 실패 상한에 도달했으면 None이다.
    pub fn pending_terminal_spawn(&self, surface_id: SurfaceId) -> Option<DeferredSpawn> {
        let empty = self.deferred_terminal_placeholder(surface_id)?;
        if empty.spawn_attempts >= Self::MAX_SPAWN_ATTEMPTS {
            return None;
        }
        empty.deferred_spawn().cloned()
    }

    /// PTY 생성에 성공한 터미널 placeholder를 `TerminalSurface` marker로 교체한다.
    /// 터미널 placeholder가 아니면 false다.
    pub fn complete_terminal_spawn(&mut self, surface_id: SurfaceId) -> bool {
        let Some(layout) = self.layout_opt.as_mut() else {
            return false;
        };
        let Some(leaf) = layout.find_leaf_mut(surface_id) else {
            return false;
        };
        let is_terminal_placeholder = leaf
            .as_any()
            .downcast_ref::<super::EmptySurface>()
            .is_some_and(|e| e.deferred_spawn().is_some());
        if !is_terminal_placeholder {
            return false;
        }
        *leaf = Box::new(TerminalSurface { id: surface_id });
        true
    }

    /// PTY 생성 실패를 기록한다. placeholder와 복원 정보는 재시도를 위해 남긴다.
    pub fn record_terminal_spawn_failure(&mut self, surface_id: SurfaceId, error: &str) {
        let Some(layout) = self.layout_opt.as_mut() else {
            return;
        };
        let Some(empty) = layout
            .find_leaf_mut(surface_id)
            .and_then(|leaf| leaf.as_any_mut().downcast_mut::<super::EmptySurface>())
            .filter(|e| e.deferred_spawn().is_some())
        else {
            return;
        };
        empty.spawn_attempts = empty.spawn_attempts.saturating_add(1);
        if empty.spawn_attempts == Self::MAX_SPAWN_ATTEMPTS {
            tracing::error!(
                "surface {surface_id}: PTY spawn {}회 연속 실패 — 재시도 중단: {error}",
                Self::MAX_SPAWN_ATTEMPTS
            );
        } else if empty.spawn_attempts == 1 {
            tracing::warn!("surface {surface_id}: PTY spawn 실패 (재시도 예정): {error}");
        }
    }

    fn deferred_terminal_placeholder(&self, surface_id: SurfaceId) -> Option<&super::EmptySurface> {
        self.layout_opt
            .as_ref()?
            .find_surface(surface_id)?
            .as_any()
            .downcast_ref::<super::EmptySurface>()
            .filter(|e| e.deferred_spawn().is_some())
    }

    /// 호스트의 restore(kind, snapshot) 콜백으로 plugin placeholder를 교체한다.
    /// None이면 그대로 남아 다음 reify에서 다시 시도한다. PTY 실패 횟수 제한과는 별개다.
    /// plugin placeholder가 아니면 false다.
    pub fn reify_deferred_plugin<F>(&mut self, surface_id: SurfaceId, restore: F) -> bool
    where
        F: FnOnce(&str, &serde_json::Value) -> Option<Box<dyn Surface>>,
    {
        let Some(layout) = self.layout_opt.as_mut() else {
            return false;
        };
        let Some(leaf) = layout.find_leaf_mut(surface_id) else {
            return false;
        };
        let (kind, snapshot) = {
            let Some(es) = leaf.as_any().downcast_ref::<super::EmptySurface>() else {
                return false;
            };
            let Some(p) = es.deferred_plugin() else {
                return false;
            };
            (p.kind.clone(), p.snapshot.clone())
        };
        let Some(new_surface) = restore(&kind, &snapshot) else {
            return false;
        };
        *leaf = new_surface;
        true
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

    /// surface_id가 지연 placeholder면 그 실제화 경로의 종류. 아니면 None.
    pub fn deferred_kind(&self, surface_id: SurfaceId) -> Option<super::DeferredKind> {
        self.layout_opt
            .as_ref()?
            .find_surface(surface_id)?
            .as_any()
            .downcast_ref::<super::EmptySurface>()?
            .deferred
            .as_ref()
            .map(super::Deferred::kind)
    }

    /// Replace the entire layout with a single surface.
    pub fn put_surface(&mut self, surface: Box<dyn Surface>) {
        self.put_layout(SurfaceLayout::Leaf(surface));
    }

    // ── Split operations ──

    /// Split a specific surface by ID with a TerminalSurface marker. Does NOT
    /// change focused_surface. Caller must have already inserted the spawned
    /// Terminal into `CoreState::runtime.terminals`.
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
    pub fn to_tree_json(
        &self,
        presentation: &(impl crate::StructurePresentation + ?Sized),
    ) -> serde_json::Value {
        let selected_surface = presentation.surface_id(self);
        let layout_json = if self.is_split() {
            let mut v = serde_json::json!({
                "type": "SplitLayout",
                "focused_surface": selected_surface.or_else(|| self.first_surface_id()).unwrap_or(0),
                "surfaces": self.all_surface_ids(),
            });
            // 분할 방향/비율/상위-하위 중첩 구조를 보존한 전체 트리. CLI `list tree`
            // 가 이걸로 split tab 의 SurfaceGroup 계층을 그린다. flat `surfaces` 는
            // 호환을 위해 남겨둔다.
            if let Some(layout) = self.layout_if_initialized() {
                v["layout"] = layout.to_tree_json_full(presentation);
            }
            v
        } else {
            // Single-leaf tab. EmptySurface(deferred) renders itself with pty_ready: false.
            // For a live TerminalSurface, append pty_ready: true.
            let mut v = self
                .surface(self.first_surface_id().expect("single leaf has an ID"))
                .expect("single leaf ID exists")
                .to_tree_json();
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
            "name": self.display_name(selected_surface),
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

    fn deferred_spawn(shell: Option<&str>) -> DeferredSpawn {
        DeferredSpawn {
            shell: shell.map(|s| s.to_string()),
            shell_args: Vec::new(),
            extra_env: Vec::new(),
            cols: 80,
            rows: 24,
            working_dir: None,
            restore_command: None,
            scrollback_persist_id: None,
        }
    }

    fn deferred_tab(sid: SurfaceId, spawn: DeferredSpawn) -> Tab {
        let surface: Box<dyn Surface> = Box::new(EmptySurface::new_deferred(sid, spawn));
        Tab::new_with_surface(1, "t".to_string(), surface)
    }

    fn deferred_plugin_tab(sid: SurfaceId, kind: &str) -> Tab {
        let surface: Box<dyn Surface> = Box::new(EmptySurface::new_deferred_plugin(
            sid,
            crate::terminal_surface::DeferredPlugin {
                kind: kind.to_string(),
                snapshot: serde_json::json!({ "x": 1 }),
            },
        ));
        Tab::new_with_surface(1, "t".to_string(), surface)
    }

    #[test]
    fn reify_deferred_plugin_replaces_when_restore_succeeds() {
        let sid = 7;
        let mut tab = deferred_plugin_tab(sid, "markdown");
        let ok = tab.reify_deferred_plugin(sid, |kind, snap| {
            assert_eq!(kind, "markdown");
            assert_eq!(snap, &serde_json::json!({ "x": 1 }));
            Some(Box::new(EmptySurface::new(sid)))
        });
        assert!(ok, "restore 가 Some 이면 교체돼야");
        let es = tab
            .surface(sid)
            .unwrap()
            .as_any()
            .downcast_ref::<EmptySurface>()
            .expect("still an EmptySurface (실제화 대역)");
        assert!(
            es.deferred_plugin().is_none(),
            "더 이상 plugin placeholder 아님"
        );
        assert!(!es.is_deferred());
    }

    // kind 등록을 기다리는 동안에는 재시도 가능한 placeholder와 ready:false를 유지한다.
    #[test]
    fn reify_deferred_plugin_keeps_placeholder_when_kind_missing() {
        let sid = 7;
        let mut tab = deferred_plugin_tab(sid, "markdown");
        let ok = tab.reify_deferred_plugin(sid, |_, _| None);
        assert!(!ok, "kind 미등록이면 교체 안 함");
        let es = tab
            .surface(sid)
            .unwrap()
            .as_any()
            .downcast_ref::<EmptySurface>()
            .expect("placeholder 유지");
        assert_eq!(
            es.deferred_plugin().map(|p| p.kind.as_str()),
            Some("markdown"),
            "kind/snapshot 이 보존돼 다음 reify 에서 재시도 가능"
        );
    }

    /// PTY 생성 실패 뒤에도 복원 정보가 남아 다시 시도할 수 있어야 한다.
    #[test]
    fn spawn_failure_keeps_surface_deferred() {
        let sid: SurfaceId = 42;
        let mut tab = deferred_tab(sid, deferred_spawn(Some("/bin/sh")));
        assert!(tab.is_surface_deferred(sid));

        let spec = tab.pending_terminal_spawn(sid).expect("터미널 placeholder");
        assert_eq!(spec.shell.as_deref(), Some("/bin/sh"));
        tab.record_terminal_spawn_failure(sid, "spawn failed");
        assert!(
            tab.is_surface_deferred(sid),
            "생성 실패 뒤에도 재시도에 필요한 복원 정보를 유지해야 한다"
        );
        assert_eq!(spawn_attempts_of(&tab, sid), Some(1));
        assert!(
            tab.pending_terminal_spawn(sid).is_some(),
            "상한 전에는 재시도"
        );
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
    /// spawn 정보를 내주지 않는다 (reify 매 프레임 폭주 차단). placeholder 는 남는다.
    #[test]
    fn pending_spawn_stops_after_max_attempts() {
        let sid: SurfaceId = 99;
        let mut tab = deferred_tab(sid, deferred_spawn(None));

        for _ in 0..(Tab::MAX_SPAWN_ATTEMPTS + 3) {
            if tab.pending_terminal_spawn(sid).is_some() {
                tab.record_terminal_spawn_failure(sid, "spawn failed");
            }
        }

        assert_eq!(
            spawn_attempts_of(&tab, sid),
            Some(Tab::MAX_SPAWN_ATTEMPTS),
            "spawn_attempts 가 MAX 에서 capped 되어야 함"
        );
        assert!(tab.pending_terminal_spawn(sid).is_none());
        assert!(tab.is_surface_deferred(sid));
    }

    #[test]
    fn complete_terminal_spawn_replaces_leaf() {
        let sid: SurfaceId = 7;
        let mut tab = deferred_tab(sid, deferred_spawn(None));
        assert!(tab.is_surface_deferred(sid));

        assert!(tab.complete_terminal_spawn(sid));
        assert!(
            !tab.is_surface_deferred(sid),
            "성공 시 deferred 해제 + TerminalSurface 로 교체"
        );
        assert!(
            tab.surface(sid)
                .unwrap()
                .as_any()
                .downcast_ref::<TerminalSurface>()
                .is_some()
        );
        assert!(tab.pending_terminal_spawn(sid).is_none());
        assert!(!tab.complete_terminal_spawn(sid), "이미 교체된 leaf");
    }

    #[test]
    fn deferred_kind_distinguishes_terminal_and_plugin() {
        use crate::DeferredKind;
        let tab = deferred_tab(1, deferred_spawn(None));
        assert_eq!(tab.deferred_kind(1), Some(DeferredKind::Terminal));
        assert_eq!(tab.deferred_kind(2), None);
        let tab = deferred_plugin_tab(3, "markdown");
        assert_eq!(tab.deferred_kind(3), Some(DeferredKind::Plugin));
        let tab = Tab::new_with_surface(1, "t".to_string(), Box::new(EmptySurface::new(4)));
        assert_eq!(tab.deferred_kind(4), None, "비-deferred 빈 surface");
    }

    /// plugin placeholder 는 터미널 spawn 경로가 건드리지 않는다.
    #[test]
    fn terminal_spawn_api_ignores_plugin_placeholder() {
        let sid = 7;
        let mut tab = deferred_plugin_tab(sid, "markdown");
        assert!(tab.pending_terminal_spawn(sid).is_none());
        tab.record_terminal_spawn_failure(sid, "x");
        assert_eq!(spawn_attempts_of(&tab, sid), Some(0));
        assert!(!tab.complete_terminal_spawn(sid));
        assert!(tab.is_surface_deferred(sid));
    }
}
