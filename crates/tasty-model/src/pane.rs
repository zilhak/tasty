use super::tab::Tab;
use super::{PaneId, SplitDirection, SurfaceId, TabId, TerminalSurface};
use tasty_terminal::{Terminal, Waker};

/// Pane 의 shell-spawning 함수들이 공유하는 옵션 묶음.
///
/// `new_with_shell`, `add_tab_with_shell`, `add_tab_background_with_shell`,
/// `split_active_surface_with_shell`,
/// `split_surface_by_id_with_shell` 함수들이 모두 `(cols, rows, shell,
/// shell_args, waker, working_dir)` 6 인자를 가져 너무 많았다 (`too_many_arguments`).
/// 공통 옵션을 struct 로 묶어 각 함수 시그니처 인자 수를 lint 임계값 내로 줄임.
pub struct ShellSpawnOpts<'a> {
    pub cols: usize,
    pub rows: usize,
    pub shell: Option<&'a str>,
    pub shell_args: &'a [&'a str],
    pub waker: Waker,
    pub working_dir: Option<&'a std::path::Path>,
    /// 자식 셸에 추가로 심을 환경변수(docs/features/terminal-output/index.md#명령-인덱싱-osc-133,
    /// `ShellConfig::envs_ref` 참고).
    pub extra_env: &'a [(&'a str, &'a str)],
}

/// A screen region with its own independent tab bar.
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    /// Horizontal scroll offset for the tab bar (in logical pixels).
    pub tab_scroll_offset: f32,
}

impl Default for Pane {
    fn default() -> Self {
        Self {
            id: 0,
            tabs: Vec::new(),
            active_tab: 0,
            tab_scroll_offset: 0.0,
        }
    }
}

impl Pane {
    /// Create a Pane with a Surface trait object.
    pub fn new_with_surface(
        id: PaneId,
        tab_id: TabId,
        name: String,
        surface: Box<dyn super::Surface>,
    ) -> Self {
        let tab = super::tab::Tab::new_with_surface(tab_id, name, surface);
        Self {
            id,
            tabs: vec![tab],
            active_tab: 0,
            tab_scroll_offset: 0.0,
        }
    }

    /// Spawn a Terminal with the given shell spawn options. Caller registers the
    /// returned Terminal into `CoreState::terminals` *before* it inserts the
    /// returned Pane (or its surface) into a workspace — so the layout never sees
    /// a missing-store-entry state.
    pub fn spawn_terminal(
        surface_id: SurfaceId,
        spawn: ShellSpawnOpts<'_>,
    ) -> anyhow::Result<Terminal> {
        Terminal::new(
            tasty_terminal::TerminalConfig {
                cols: spawn.cols,
                rows: spawn.rows,
                shell: spawn.shell,
                args: spawn.shell_args,
                surface_id,
                working_dir: spawn.working_dir,
                initial_input: None,
                extra_env: spawn.extra_env,
            },
            spawn.waker,
        )
    }

    /// Create a Pane with a TerminalSurface marker. Caller must have already
    /// `engine.terminals.insert(surface_id, terminal)` for the spawned Terminal.
    pub fn new_with_terminal_marker(id: PaneId, tab_id: TabId, surface_id: SurfaceId) -> Self {
        let surface: Box<dyn super::Surface> = Box::new(TerminalSurface { id: surface_id });
        let tab = Tab::new_with_surface(tab_id, "Shell".to_string(), surface);
        Self {
            id,
            tabs: vec![tab],
            active_tab: 0,
            tab_scroll_offset: 0.0,
        }
    }

    /// Add a TerminalSurface-marker tab (active). Caller must have already
    /// inserted the spawned Terminal into the store.
    pub fn add_terminal_marker_tab(&mut self, tab_id: TabId, surface_id: SurfaceId) {
        let surface: Box<dyn super::Surface> = Box::new(TerminalSurface { id: surface_id });
        let tab = Tab::new_with_surface(tab_id, "Shell".to_string(), surface);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    /// Same as [`add_terminal_marker_tab`] but does NOT change `active_tab`.
    pub fn add_terminal_marker_tab_background(
        &mut self,
        tab_id: TabId,
        surface_id: SurfaceId,
        explicit_name: Option<String>,
    ) {
        let surface: Box<dyn super::Surface> = Box::new(TerminalSurface { id: surface_id });
        let tab = Tab::new_named(tab_id, "Shell".to_string(), explicit_name, surface);
        self.tabs.push(tab);
    }

    /// Collect all surface IDs across all tabs in this pane.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        let mut ids = Vec::new();
        for tab in &self.tabs {
            ids.extend(tab.all_surface_ids());
        }
        ids
    }

    /// 활성 tab 안의 모든 deferred placeholder를 spawn. 반환은
    /// `(surface_id, Terminal, persist_id)` 목록 — caller 가 store insert.
    pub fn ensure_active_tab_initialized_all(
        &mut self,
    ) -> Vec<(SurfaceId, Terminal, Option<String>)> {
        if self.tabs.is_empty() {
            return Vec::new();
        }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        self.tabs[idx].ensure_all_initialized()
    }

    /// Split the active panel's focused surface with a TerminalSurface marker.
    /// Caller must have already inserted the spawned Terminal into the store.
    pub fn split_active_surface_marker(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
    ) {
        if self.tabs.is_empty() {
            return;
        }
        let active = self.active_tab.min(self.tabs.len() - 1);
        self.tabs[active].split_focused_surface(direction, new_surface_id);
    }

    /// Split a specific surface by ID with a TerminalSurface marker. Caller must
    /// have already inserted the spawned Terminal into the store.
    pub fn split_surface_by_id_marker(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
    ) -> anyhow::Result<()> {
        for tab in &mut self.tabs {
            if tab.contains_surface(target_surface_id) {
                tab.split_surface_by_id(target_surface_id, direction, new_surface_id);
                return Ok(());
            }
        }
        anyhow::bail!("surface {} not found in this pane", target_surface_id)
    }

    /// Split a specific surface by ID with any surface type (not just terminal).
    pub fn split_surface_by_id_with_surface(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface: Box<dyn super::Surface>,
    ) -> anyhow::Result<()> {
        for tab in &mut self.tabs {
            if tab.contains_surface(target_surface_id) {
                tab.split_surface_by_id_generic(target_surface_id, direction, new_surface);
                return Ok(());
            }
        }
        anyhow::bail!("surface {} not found in this pane", target_surface_id)
    }

    /// Remove the tab at `tab_index`, keeping `active_tab` pointed at the **same
    /// tab** it pointed at before.
    ///
    /// `active_tab` is an index, so removing an earlier tab shifts every later
    /// tab down one slot and the untouched index silently starts naming a
    /// different tab. Only closing the active tab itself may move the view, and
    /// then it lands on the tab that slid into the slot (or the last one).
    /// See `docs/design/policies/focus.md`.
    pub fn remove_tab_preserving_active(&mut self, tab_index: usize) {
        if tab_index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(tab_index);
        if tab_index < self.active_tab {
            self.active_tab -= 1;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len().saturating_sub(1);
        }
    }

    /// Close the tab at the given index. Returns false if the tab can't be closed
    /// (e.g., it's the last tab).
    pub fn close_tab(&mut self, tab_index: usize) -> bool {
        if self.tabs.len() <= 1 {
            return false; // Can't close last tab
        }
        if tab_index < self.tabs.len() {
            self.remove_tab_preserving_active(tab_index);
            true
        } else {
            false
        }
    }

    /// Close the currently active tab. Returns false if it's the last tab.
    pub fn close_active_tab(&mut self) -> bool {
        self.close_tab(self.active_tab)
    }

    /// Close a tab by its ID. Returns false if not found or it's the last tab.
    pub fn close_tab_by_id(&mut self, tab_id: TabId) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        if let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) {
            self.remove_tab_preserving_active(idx);
            true
        } else {
            false
        }
    }

    /// Check if any tab in this pane contains the given surface ID.
    pub fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        self.tabs.iter().any(|tab| tab.contains_surface(surface_id))
    }

    /// Switch to tab by index (0-based). Returns true if switched.
    pub fn goto_tab(&mut self, index: usize) -> bool {
        if index < self.tabs.len() && index != self.active_tab {
            self.active_tab = index;
            true
        } else {
            false
        }
    }

    /// Switch to next tab.
    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Switch to previous tab.
    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    /// Add a tab with a Surface trait object and switch to it.
    pub fn add_surface_tab(
        &mut self,
        tab_id: TabId,
        name: String,
        explicit_name: Option<String>,
        surface: Box<dyn super::Surface>,
    ) {
        let tab = super::tab::Tab::new_named(tab_id, name, explicit_name, surface);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    /// Get the active tab (mutable). Returns None if tabs are empty.
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        if self.tabs.is_empty() {
            return None;
        }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        Some(&mut self.tabs[idx])
    }

    /// Move a tab from one index to another, adjusting active_tab accordingly.
    /// Returns false if indices are out of bounds or equal.
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        // Adjust active_tab to follow the moved tab or account for the shift
        if self.active_tab == from {
            self.active_tab = to;
        } else if from < to && self.active_tab > from && self.active_tab <= to {
            self.active_tab -= 1;
        } else if from > to && self.active_tab >= to && self.active_tab < from {
            self.active_tab += 1;
        }
        true
    }

    /// Produce a JSON tree representation of this pane.
    pub fn to_tree_json(&self) -> serde_json::Value {
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let mut t = tab.to_tree_json();
                t["active"] = serde_json::json!(i == self.active_tab);
                t
            })
            .collect();
        serde_json::json!({
            "id": self.id,
            "tabs": tabs,
        })
    }

    /// attach 디스크립터용 pane JSON: `{"id", "tabs":[{"id","name","active",
    /// "focused_surface","layout"}, ...]}`. `Workspace::to_attach_tree_json`(평면
    /// "panes")과 `PaneNode::to_tree_json_full`(트리 Leaf)이 이 메서드를 공유해
    /// 두 표현이 서로 다른 pane 직렬화를 갖지 않도록 한다.
    pub fn to_attach_json(&self) -> serde_json::Value {
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let layout = tab
                    .layout_if_initialized()
                    .map(|l| l.to_tree_json_full())
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "id": tab.id,
                    "name": tab.display_name(),
                    "active": i == self.active_tab,
                    "focused_surface": tab.focused_surface,
                    "layout": layout,
                })
            })
            .collect();
        serde_json::json!({
            "id": self.id,
            "tabs": tabs,
        })
    }
}

#[cfg(test)]
mod tab_removal_focus_tests {
    //! 탭 제거가 `active_tab` 을 **대상 기준으로** 보존하는지 고정한다.
    //!
    //! `active_tab` 은 인덱스가 진실 소스라, 앞쪽 탭이 빠지면 손대지 않은 인덱스가
    //! 다른 탭을 가리키게 된다 — 사용자가 아무 조작도 하지 않았는데 보던 탭이
    //! 바뀌는 것이라 불가침 원칙 1 위반이다. 단정은 인덱스가 아니라 **탭 id** 로 한다.
    use super::*;

    /// 탭 3 개(id 10/11/12)를 가진 pane. `active_tab` 은 호출자가 정한다.
    fn pane_with_three_tabs(active: usize) -> Pane {
        let mut pane = Pane::new_with_terminal_marker(1, 10, 100);
        pane.add_terminal_marker_tab(11, 101);
        pane.add_terminal_marker_tab(12, 102);
        pane.active_tab = active;
        pane
    }

    fn active_tab_id(pane: &Pane) -> TabId {
        pane.tabs[pane.active_tab].id
    }

    #[test]
    fn removing_an_earlier_tab_keeps_the_same_tab_active() {
        let mut pane = pane_with_three_tabs(1); // 사용자는 tab 11 을 본다
        pane.remove_tab_preserving_active(0);
        assert_eq!(
            active_tab_id(&pane),
            11,
            "앞쪽 탭 제거가 보던 탭을 바꾸면 안 된다"
        );
    }

    #[test]
    fn removing_a_later_tab_keeps_the_same_tab_active() {
        let mut pane = pane_with_three_tabs(1);
        pane.remove_tab_preserving_active(2);
        assert_eq!(active_tab_id(&pane), 11);
    }

    #[test]
    fn removing_the_active_tab_lands_on_the_tab_that_slid_in() {
        let mut pane = pane_with_three_tabs(1);
        pane.remove_tab_preserving_active(1);
        assert_eq!(
            active_tab_id(&pane),
            12,
            "보던 탭 자체를 닫으면 그 자리로 밀려 들어온 탭으로 간다"
        );
    }

    #[test]
    fn removing_the_active_last_tab_falls_back_to_the_previous_one() {
        let mut pane = pane_with_three_tabs(2);
        pane.remove_tab_preserving_active(2);
        assert_eq!(active_tab_id(&pane), 11);
    }

    #[test]
    fn close_tab_by_index_preserves_the_active_tab() {
        // active 는 **가운데**(1)여야 한다. 마지막(2)이면 수정 전의 범위 초과 clamp
        // 로도 우연히 같은 탭에 착지해 이 wrapper 가 헬퍼를 타는지 판별하지 못한다.
        let mut pane = pane_with_three_tabs(1); // 사용자는 tab 11 을 본다
        assert!(pane.close_tab(0));
        assert_eq!(active_tab_id(&pane), 11);
    }

    #[test]
    fn close_tab_by_id_preserves_the_active_tab() {
        let mut pane = pane_with_three_tabs(1);
        assert!(pane.close_tab_by_id(10));
        assert_eq!(active_tab_id(&pane), 11);
    }

    #[test]
    fn removing_an_out_of_range_index_is_a_noop() {
        let mut pane = pane_with_three_tabs(1);
        pane.remove_tab_preserving_active(9);
        assert_eq!(pane.tabs.len(), 3);
        assert_eq!(active_tab_id(&pane), 11);
    }
}
