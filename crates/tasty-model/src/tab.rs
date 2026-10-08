use super::SurfaceDescriptor;
use super::surface_layout::SurfaceLayout;
use super::{SplitDirection, SurfaceId, TabId};

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
}

impl Tab {
    /// Create a tab with a Surface trait object.
    pub fn new_with_surface(id: TabId, name: String, surface: SurfaceDescriptor) -> Self {
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
        surface: SurfaceDescriptor,
    ) -> Self {
        Self {
            id,
            name,
            explicit_name,
            layout_opt: Some(SurfaceLayout::Leaf(surface)),
        }
    }

    /// Get the display name for this tab (cached, no syscalls).
    /// Priority: explicit_name > osc_title > cached CWD-derived name > fallback "name" field.
    pub fn display_name(&self, observed: Option<&SurfaceTitle>) -> String {
        if let Some(ref explicit) = self.explicit_name {
            return explicit.clone();
        }
        if let Some(title) = observed {
            if let Some(osc) = &title.osc_title {
                return osc.clone();
            }
            if let Some(cwd) = &title.cwd_name {
                return cwd.clone();
            }
        }
        self.name.clone()
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

    /// Find exactly the requested surface; a stale ID never selects a sibling.
    pub fn surface(&self, surface_id: SurfaceId) -> Option<&SurfaceDescriptor> {
        self.layout_opt.as_ref()?.find_surface(surface_id)
    }

    pub fn surface_mut(&mut self, surface_id: SurfaceId) -> Option<&mut SurfaceDescriptor> {
        match self.layout_opt.as_mut()?.find_leaf_mut(surface_id) {
            Some(surface) => Some(surface),
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
    pub fn for_each_surface(&self, f: &mut dyn FnMut(&SurfaceDescriptor)) {
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

    /// All surface regions with Surface trait references.
    pub fn surface_regions(&self, rect: super::PhysicalRect) -> Vec<super::SurfaceRegion<'_>> {
        self.layout().surface_regions(rect)
    }

    /// Replace the entire layout with a single surface.
    pub fn put_surface(&mut self, surface: SurfaceDescriptor) {
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
        let new_node = SurfaceDescriptor::new(new_surface_id, "terminal");
        let old_layout = self.take_layout();
        let (new_layout, remaining) =
            old_layout.split_with_surface(target_surface_id, direction, new_node);
        self.put_layout(new_layout);
        remaining.is_none()
    }

    /// Split a specific surface by ID with any surface type.
    pub fn split_surface_by_id_generic(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface: SurfaceDescriptor,
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
        surface_json: &dyn Fn(SurfaceId) -> serde_json::Value,
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
            // Single-leaf tab. For a live TerminalSurface, append pty_ready: true unless the
            // caller's surface_json already set it.
            let mut v = surface_json(self.first_surface_id().expect("single leaf has an ID"));
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
            "name": self.display_name(selected_surface.and_then(|id|presentation.surface_title(id))),
            "surface": layout_json,
        })
    }
}
