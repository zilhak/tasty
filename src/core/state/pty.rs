use std::time::Instant;

use tasty_terminal::{Terminal, TerminalEvent};

use super::CoreState;

impl CoreState {
    /// 유휴 TTL이 지난 등록을 지우고 Terminal과 waker 기록도 함께 정리한다.
    /// 실제 자식 종료·회수는 Terminal의 소유권과 플랫폼별 Drop 처리에 달려 있다.
    pub(crate) fn sweep_idle_ptys(&mut self, now: Instant) -> Vec<u32> {
        let expired = self.pty_registry.sweep_idle(now);
        for pty_id in &expired {
            let pty_id = *pty_id;
            self.terminals.remove(pty_id);
            if let Some(factory) = self.waker_factory.as_ref() {
                factory.forget_surface(pty_id);
            }
            tracing::debug!("headless pty {pty_id} swept (idle TTL exceeded)");
        }
        expired
    }

    pub fn send_fast_init(&mut self, surface_id: u32) {
        if let Err(e) = crate::surface_meta::SurfaceMetaStore::ensure_created(surface_id) {
            tracing::warn!("surface_meta ensure_created failed for surface {surface_id}: {e}");
        }
        let scrollback_limit = self.settings.general.scrollback_lines;
        let disk_swap = self.settings.performance.scrollback_disk_swap;
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.set_scrollback_limit(scrollback_limit);
            if disk_swap {
                terminal.enable_disk_scrollback(surface_id);
            }
        }
        // bash 초기화 파일은 rcfile 인자로 전달한다. 여기서는 사용자 startup_command만 입력한다.
        let startup = self.settings.general.startup_command.trim();
        if !startup.is_empty() {
            let line = format!("{startup}\n");
            if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
                terminal.send_key(&line);
            }
        }
    }

    pub fn is_surface_deferred(&self, surface_id: u32) -> bool {
        for ws in &self.workspaces {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        if tab.is_surface_deferred(surface_id) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// mesh 메타데이터를 찾는다. 허용 종류·plugin 검증은 attach 호출자가 따로 한다.
    pub(crate) fn find_mesh_surface_info(&self, surface_id: u32) -> Option<(String, String)> {
        for ws in &self.workspaces {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        if let Some(layout) = tab.layout_if_initialized()
                            && let Some(surface) = layout.find_surface(surface_id)
                            && let Some((kind, plugin_id)) = surface.attach_mesh_info()
                        {
                            return Some((kind.to_string(), plugin_id.to_string()));
                        }
                    }
                }
            }
        }
        None
    }

    /// attach의 surface.create에 필요한 파일·표시 이름까지 얻으려고 host의 mesh 타입을 조회한다.
    pub(crate) fn find_egui_mesh_surface(
        &self,
        surface_id: u32,
    ) -> Option<&crate::core::egui_mesh_surface::EguiMeshSurface> {
        for ws in &self.workspaces {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        if let Some(layout) = tab.layout_if_initialized()
                            && let Some(surface) = layout.find_surface(surface_id)
                            && let Some(ms) = surface
                                .as_any()
                                .downcast_ref::<crate::core::egui_mesh_surface::EguiMeshSurface>(
                            )
                        {
                            return Some(ms);
                        }
                    }
                }
            }
        }
        None
    }

    /// deferred PTY 생성에 성공하면 true다. 포커스·활성 workspace·활성 tab은 바꾸지 않는다.
    /// model placeholder에서 생성 정보를 읽고, PTY와 waker는 여기서 만든 뒤 결과를 model에 반영한다.
    pub fn ensure_surface_initialized(&mut self, surface_id: u32) -> bool {
        let Some(spawn) = self
            .deferred_tab_mut(surface_id)
            .and_then(|tab| tab.pending_terminal_spawn(surface_id))
        else {
            return false;
        };
        let waker = self.make_waker(surface_id);
        let result =
            crate::core::terminal_spawn::spawn_deferred_terminal(surface_id, &spawn, waker);
        let Some(tab) = self.deferred_tab_mut(surface_id) else {
            return false;
        };
        let terminal = match result {
            Ok(terminal) => terminal,
            Err(e) => {
                tab.record_terminal_spawn_failure(surface_id, &e.to_string());
                return false;
            }
        };
        if !tab.complete_terminal_spawn(surface_id) {
            return false;
        }
        self.terminals.insert(surface_id, terminal);
        if let Some(pid) = spawn.scrollback_persist_id {
            self.terminals.set_scrollback_persist_id(surface_id, pid);
        }
        self.send_fast_init(surface_id);
        self.apply_pending_scrollback_inject(surface_id);
        true
    }

    /// surface_id를 지연 placeholder로 가진 tab. 실패 상한에 도달한 placeholder도 찾는다.
    fn deferred_tab_mut(&mut self, surface_id: u32) -> Option<&mut crate::model::Tab> {
        let (ws_idx, pane_id, tab_idx) =
            self.workspaces.iter().enumerate().find_map(|(i, ws)| {
                ws.pane_layout()
                    .all_pane_ids()
                    .into_iter()
                    .find_map(|pane_id| {
                        let pane = ws.pane_layout().find_pane(pane_id)?;
                        let tab_idx = pane
                            .tabs
                            .iter()
                            .position(|tab| tab.is_surface_deferred(surface_id))?;
                        Some((i, pane_id, tab_idx))
                    })
            })?;
        self.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)?
            .tabs
            .get_mut(tab_idx)
    }

    /// 대기 중인 scrollback을 꺼내 Terminal에 적용한다. Terminal이 없으면 꺼낸 내용은 버린다.
    /// 이미 시작한 PTY의 첫 출력보다 먼저 적용된다고 보장하지는 않는다.
    pub fn apply_pending_scrollback_inject(&mut self, surface_id: u32) {
        let Some(lines) = self.pending_scrollback_inject.remove(&surface_id) else {
            return;
        };
        if lines.is_empty() {
            return;
        }
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.inject_scrollback(lines);
            let prefill = terminal.rows() / 2;
            terminal.prefill_visible_from_scrollback(prefill);
        }
    }

    /// 트리는 유지하고 store의 Terminal을 교체한 뒤 기존 Terminal을 drop한다.
    /// 기존 ID가 없으면 새 Terminal을 등록한 상태에서 Err를 반환한다.
    pub fn replace_terminal_by_id(
        &mut self,
        surface_id: u32,
        new_terminal: Terminal,
    ) -> anyhow::Result<()> {
        if let Some(old) = self.terminals.replace(surface_id, new_terminal) {
            drop(old);
            return Ok(());
        }
        anyhow::bail!("Surface {} not found", surface_id)
    }

    /// OutputAppended 발생 여부를 현재 observer 목록에 맞춘다. 생성 직후만 설정하면 등록 변경을 놓친다.
    pub(crate) fn sync_output_event_gates(&mut self) {
        let router = &self.observer_router;
        let hook_manager = &self.hook_manager;
        for (sid, t) in self.terminals.iter_mut() {
            t.set_output_events_enabled(
                router.wants(sid) || hook_manager.has_output_match_hook(sid),
            );
        }
    }

    pub fn process_all(&mut self) -> bool {
        self.sync_output_event_gates();
        self.terminals.process_all()
    }

    /// Windows 절전 복귀 후 살아 있는 자식에 wake를 시도하고 비셸 전경 surface를 알림 후보로 반환한다.
    /// 실제 응답 회복을 확인한 결과는 아니다.
    #[cfg(all(windows, feature = "gui"))]
    pub(crate) fn wake_terminals_after_resume(&mut self) -> Vec<u32> {
        let mut suspects = Vec::new();
        for (sid, term) in self.terminals.iter_mut() {
            if !term.check_process_alive() {
                continue; // 죽음 — process_all 의 ProcessExited cascade 가 정리.
            }
            term.wake_nudge();
            let shell_pid = term.process_id();
            if let Some(info) = term.foreground_process_info()
                && Some(info.pid) != shell_pid
                && !tasty_terminal::foreground_process::is_known_shell_name(&info.name)
            {
                suspects.push(sid);
            }
        }
        suspects
    }

    pub fn process_surface(&mut self, surface_id: u32) -> bool {
        let enabled = self.observer_router.wants(surface_id)
            || self.hook_manager.has_output_match_hook(surface_id);
        if let Some(t) = self.terminals.get_mut(surface_id) {
            t.set_output_events_enabled(enabled);
        }
        self.terminals.process_surface(surface_id)
    }

    #[cfg(feature = "gui")]
    pub fn flush_all_pty_resizes(&mut self) -> bool {
        self.terminals.flush_pty_resizes()
    }

    pub fn mark_layout_dirty(&mut self) {
        self.layout_dirty.mark_dirty();
    }

    /// 락을 즉시 얻지 못한 Terminal은 이번 수집에서 건너뛴다.
    /// 그 이벤트는 큐에 남지만 여기서 다음 호출 시각이나 전달 성공까지 보장하지는 않는다.
    pub fn collect_events(&mut self) -> Vec<TerminalEvent> {
        let mut all_events = Vec::new();
        for (sid, terminal) in self.terminals.iter_mut() {
            let Some(mut events) = terminal.try_take_events() else {
                continue;
            };
            for event in &mut events {
                event.surface_id = sid;
            }
            all_events.extend(events);
        }
        all_events
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    // 이유: 현재 호출자가 없으며 터미널 ID 조회 API를 유지한다.
    #[allow(dead_code)]
    pub fn all_terminal_surface_ids(&mut self) -> Vec<u32> {
        self.terminals.iter().map(|(id, _)| id).collect()
    }
}

/// 종류별 지연 surface 실제화. plugin restore는 GUI 경로와 시험에서만 쓴다.
#[cfg(any(feature = "gui", test))]
impl CoreState {
    /// 지연 placeholder를 종류에 맞는 경로로 실제화하는 단일 진입점이다. 성공하면 true다.
    /// 터미널은 PTY를 만들고, plugin은 등록된 kind의 restore를 호출한다.
    /// 포커스·활성 workspace·활성 tab은 바꾸지 않는다.
    pub fn reify_deferred_surface(&mut self, surface_id: u32) -> bool {
        let kind = self
            .deferred_tab_mut(surface_id)
            .and_then(|tab| tab.deferred_kind(surface_id));
        match kind {
            Some(crate::model::DeferredKind::Terminal) => {
                self.ensure_surface_initialized(surface_id)
            }
            Some(crate::model::DeferredKind::Plugin) => self.reify_plugin_surface(surface_id),
            None => false,
        }
    }

    /// 등록된 종류로 placeholder 복원을 시도한다. 종류가 없거나 복원에 실패하면 placeholder가 남는다.
    pub fn reify_plugin_surface(&mut self, surface_id: u32) -> bool {
        let registry = self.surface_registry.clone();
        for ws in &mut self.workspaces {
            let pane_ids: Vec<u32> = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    for tab in &mut pane.tabs {
                        if tab.reify_deferred_plugin(surface_id, |kind, snap| {
                            let def = registry.get_live(kind)?;
                            (def.restore)(surface_id, snap).ok()
                        }) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::CoreState;
    use crate::model::{DeferredSpawn, EmptySurface, Tab};

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// 첫 pane에 지연 터미널 탭을 넣고 surface ID를 반환한다. 활성 탭은 바꾸지 않는다.
    fn push_deferred_tab(engine: &mut CoreState, shell: Option<&str>) -> u32 {
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let spawn = DeferredSpawn {
            shell: shell.map(str::to_string),
            shell_args: Vec::new(),
            extra_env: Vec::new(),
            cols: 80,
            rows: 24,
            working_dir: None,
            restore_command: None,
            scrollback_persist_id: Some("persist-xyz".to_string()),
        };
        let tab = Tab::new_named(
            tab_id,
            "Shell".to_string(),
            None,
            Box::new(EmptySurface::new_deferred(surface_id, spawn)),
        );
        let ws = &mut engine.workspaces[0];
        let pane_id = ws.pane_layout().all_pane_ids()[0];
        let pane = ws.pane_layout_mut().find_pane_mut(pane_id).expect("pane");
        pane.tabs.push(tab);
        surface_id
    }

    fn push_deferred_plugin_tab(engine: &mut CoreState, kind: &str) -> u32 {
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let placeholder = EmptySurface::new_deferred_plugin(
            surface_id,
            crate::model::DeferredPlugin {
                kind: kind.to_string(),
                snapshot: serde_json::Value::Null,
            },
        );
        let tab = Tab::new_named(tab_id, "t".to_string(), None, Box::new(placeholder));
        let ws = &mut engine.workspaces[0];
        let pane_id = ws.pane_layout().all_pane_ids()[0];
        let pane = ws.pane_layout_mut().find_pane_mut(pane_id).expect("pane");
        pane.tabs.push(tab);
        surface_id
    }

    #[test]
    fn reify_deferred_surface_dispatches_by_kind() {
        let mut engine = engine();
        let term = push_deferred_tab(&mut engine, None);
        let plugin = push_deferred_plugin_tab(&mut engine, "empty");
        let missing = push_deferred_plugin_tab(&mut engine, "tasty_no_such_kind");
        let before = active_tab_ids(&engine);

        assert!(engine.reify_deferred_surface(term), "터미널은 PTY 생성");
        assert!(engine.terminals.get(term).is_some());
        assert!(
            engine.reify_deferred_surface(plugin),
            "등록된 kind 는 restore"
        );
        assert!(!engine.is_surface_deferred(plugin));
        assert!(engine.terminals.get(plugin).is_none(), "plugin 은 PTY 없음");
        assert!(!engine.reify_deferred_surface(missing), "미등록 kind");
        assert!(engine.is_surface_deferred(missing), "placeholder 유지");
        assert!(!engine.reify_deferred_surface(term), "이미 실제화됨");
        assert_eq!(active_tab_ids(&engine), before, "활성 탭 불변");
    }

    fn active_tab_ids(engine: &CoreState) -> Vec<(u32, usize)> {
        let ws = &engine.workspaces[0];
        ws.pane_layout()
            .all_pane_ids()
            .into_iter()
            .filter_map(|id| ws.pane_layout().find_pane(id).map(|p| (id, p.active_tab)))
            .collect()
    }

    #[test]
    fn ensure_surface_initialized_moves_persist_id_and_keeps_active_tab() {
        let mut engine = engine();
        let sid = push_deferred_tab(&mut engine, None);
        let before = active_tab_ids(&engine);

        assert!(engine.ensure_surface_initialized(sid), "기본 셸 spawn 성공");
        assert!(!engine.is_surface_deferred(sid));
        assert!(engine.terminals.get(sid).is_some(), "store 에 등록");
        assert_eq!(
            engine.terminals.scrollback_persist_id(sid),
            Some("persist-xyz")
        );
        assert_eq!(active_tab_ids(&engine), before, "활성 탭 불변");
        assert!(!engine.ensure_surface_initialized(sid), "이미 생성됨");
    }

    #[test]
    fn ensure_surface_initialized_failure_keeps_placeholder_until_the_cap() {
        let mut engine = engine();
        let sid = push_deferred_tab(&mut engine, Some("/nonexistent/tasty_no_such_shell"));

        for _ in 0..(Tab::MAX_SPAWN_ATTEMPTS + 3) {
            assert!(!engine.ensure_surface_initialized(sid));
        }
        assert!(engine.is_surface_deferred(sid), "placeholder 유지");
        assert!(engine.terminals.get(sid).is_none());
        let tab = engine.deferred_tab_mut(sid).expect("deferred tab");
        assert!(
            tab.pending_terminal_spawn(sid).is_none(),
            "상한 뒤에는 spawn 정보를 내주지 않는다"
        );
    }
}
