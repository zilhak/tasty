use crate::core::CoreState;

use super::RequestContext;
use crate::core::engine_access::EngineMut;

/// 닫기 요청 출처. 복원 사본 저장, surface.closed의 reason, 계측 구분값을 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCloseOrigin {
    /// 사용자 단축키·메뉴 또는 사용자 입력을 재현하는 debug IPC 경로.
    #[cfg(any(feature = "gui", debug_assertions, test))]
    User,
    /// workspace.close IPC/CLI 경로.
    Agent,
}

impl WorkspaceCloseOrigin {
    /// 사용자의 닫은 항목 복원 목록에 저장할지 정한다.
    fn saves_snapshot(self) -> bool {
        self.is_user()
    }

    /// surface.closed 이벤트에 User 또는 Ipc reason을 넣을 때 사용한다.
    fn is_user_close(self) -> bool {
        self.is_user()
    }

    /// User가 없는 빌드도 처리하도록 cfg가 붙은 match 분기를 사용한다.
    fn is_user(self) -> bool {
        match self {
            #[cfg(any(feature = "gui", debug_assertions, test))]
            Self::User => true,
            Self::Agent => false,
        }
    }

    /// close 계측(`close_trace`)의 경로 구분값.
    fn trace_path(self) -> &'static str {
        match self {
            #[cfg(any(feature = "gui", debug_assertions, test))]
            Self::User => "gui",
            Self::Agent => "ipc",
        }
    }
}

impl RequestContext {
    /// 워크스페이스가 없으면 기본 항목을 생성하고 true를 반환한다. 실패하면 false다.
    /// 원격 연결 해제 등으로 빈 상태가 됐을 때 사용자 창을 유지하기 위한 처리이며
    /// 별도의 host event는 만들지 않는다.
    pub(crate) fn recreate_workspace_if_empty(
        &mut self,
        engine: &mut EngineMut<'_>,
        context: &str,
    ) -> bool {
        if !engine.workspaces.is_empty() {
            return false;
        }
        match crate::core::apply_create_workspace_inner(
            engine,
            crate::core::WorkspaceCreationParams::terminal(),
        ) {
            Ok(crate::core::intent::CoreEvent::WorkspaceCreated { index, .. }) => {
                self.set_active_workspace_index(engine, index);
                true
            }
            Ok(_) => unreachable!("apply_create_workspace_inner 는 WorkspaceCreated 만 반환"),
            Err(e) => {
                tracing::warn!("{context}: auto-recreate workspace failed: {e}");
                false
            }
        }
    }

    /// 0-based 인덱스로 전환한다. 사용자 입력과 debug IPC에서만 호출한다.
    #[cfg(any(feature = "gui", debug_assertions, test))]
    pub fn switch_workspace(&mut self, engine: &mut EngineMut<'_>, index: usize) {
        if index < engine.workspaces.len() {
            self.set_active_workspace_index(engine, index);
            let cat = engine.workspaces[index].category;
            self.category_last_active
                .insert(cat, engine.workspaces[index].id);
            self.ensure_active_workspace_initialized(engine);
        }
    }

    /// 섹션 인덱스(0=normal)로 카테고리를 전환한다. 사용자 키 입력 경로다.
    /// 접힌 카테고리는 펼쳐 저장하고, 마지막으로 본 워크스페이스를 선택한다.
    /// 기록된 대상이 없거나 다른 카테고리로 이동했다면 첫 항목을 선택한다.
    #[cfg(any(feature = "gui", test))]
    pub fn switch_to_category(&mut self, engine: &mut EngineMut<'_>, section_idx: usize) {
        let Some(cat) = engine.categories().get(section_idx).map(|c| c.id) else {
            return;
        };
        let collapsed = engine
            .categories()
            .get(section_idx)
            .is_some_and(|c| self.navigation.collapsed_categories.contains(&c.id));
        if collapsed {
            self.dispatch_intent(
                crate::intent::Intent::Ui(crate::intent::UiIntent::SetCategoryCollapsed {
                    id: cat,
                    collapsed: false,
                })
                .from_user_shortcut("switch_to_category"),
            );
        }
        let target = self
            .category_last_active
            .get(&cat)
            .copied()
            .and_then(|ws_id| {
                engine
                    .workspaces
                    .iter()
                    .position(|w| w.id == ws_id && w.category == cat)
            })
            .or_else(|| {
                engine
                    .workspaces_in_category(cat)
                    .first()
                    .map(|(gi, _)| *gi)
            });
        if let Some(global) = target {
            self.switch_workspace(engine, global);
        }
    }

    /// 활성 카테고리의 local_idx를 전역 인덱스로 바꿔 전환한다.
    /// 카테고리 기능이 꺼졌거나 해당 위치가 없으면 처리하지 않는다.
    #[cfg(feature = "gui")]
    pub fn switch_workspace_in_active_category(
        &mut self,
        engine: &mut EngineMut<'_>,
        local_idx: usize,
    ) {
        if self.active_workspace_index(engine) >= engine.workspaces.len() {
            return;
        }
        let cat = engine.workspaces[self.active_workspace_index(engine)].category;
        let global = engine
            .workspaces_in_category(cat)
            .get(local_idx)
            .map(|(gi, _)| *gi);
        if let Some(global) = global {
            self.switch_workspace(engine, global);
        }
    }

    /// 활성 카테고리의 다음 워크스페이스로 이동한다. 사용자 키 입력 경로다.
    /// 마지막 항목에서는 workspace_switch_crosses_category에 따라 같은 카테고리의
    /// 처음으로 돌아가거나 다음 카테고리로 넘어간다.
    #[cfg(any(feature = "gui", test))]
    pub fn next_workspace_in_active_category(&mut self, engine: &mut EngineMut<'_>) {
        if let Some(target) = self.relative_workspace_in_active_category(engine, 1) {
            self.switch_workspace(engine, target);
        }
    }

    /// 이전 항목으로 이동한다. 경계 처리는 next_workspace_in_active_category와 반대다.
    #[cfg(any(feature = "gui", test))]
    pub fn prev_workspace_in_active_category(&mut self, engine: &mut EngineMut<'_>) {
        if let Some(target) = self.relative_workspace_in_active_category(engine, -1) {
            self.switch_workspace(engine, target);
        }
    }

    /// 활성 카테고리에서 delta(±1)만큼 이동할 대상의 전역 인덱스를 구한다.
    /// 설정에 따라 카테고리 경계를 넘고, 넘을 수 없으면 같은 카테고리에서 순환한다.
    #[cfg(any(feature = "gui", test))]
    fn relative_workspace_in_active_category(
        &self,
        engine: &CoreState,
        delta: isize,
    ) -> Option<usize> {
        if self.active_workspace_index(engine) >= engine.workspaces.len() {
            return None;
        }
        let cat = engine.workspaces[self.active_workspace_index(engine)].category;
        let locals = engine.workspaces_in_category(cat);
        let len = locals.len();
        let pos = locals
            .iter()
            .position(|(gi, _)| *gi == self.active_workspace_index(engine))?;

        if engine.settings.general.workspace_switch_crosses_category {
            let raw = pos as isize + delta;
            if raw < 0 || raw >= len as isize {
                if let Some(target) = self.relative_category_boundary_workspace(engine, delta) {
                    return Some(target);
                }
            } else {
                return Some(locals[raw as usize].0);
            }
        }

        if len <= 1 {
            return None;
        }
        let new_pos = (pos as isize + delta).rem_euclid(len as isize) as usize;
        Some(locals[new_pos].0)
    }

    /// 인접 카테고리의 첫 항목(delta > 0) 또는 마지막 항목을 선택한다.
    /// switch_to_category와 달리 마지막 조회 기록을 사용하지 않는다.
    #[cfg(any(feature = "gui", test))]
    fn relative_category_boundary_workspace(
        &self,
        engine: &CoreState,
        delta: isize,
    ) -> Option<usize> {
        let section_idx = self.relative_category_section(engine, delta)?;
        let target_cat = engine.categories().get(section_idx)?.id;
        let locals = engine.workspaces_in_category(target_cat);
        if delta > 0 {
            locals.first().map(|(gi, _)| *gi)
        } else {
            locals.last().map(|(gi, _)| *gi)
        }
    }

    /// 다음 카테고리로 순환하며, 해당 카테고리에서 마지막으로 본 항목을 선택한다.
    /// 카테고리가 하나뿐이면 처리하지 않는다. 사용자 키 입력 경로다.
    #[cfg(any(feature = "gui", test))]
    pub fn next_category(&mut self, engine: &mut EngineMut<'_>) {
        if let Some(section_idx) = self.relative_category_section(engine, 1) {
            self.switch_to_category(engine, section_idx);
        }
    }

    #[cfg(any(feature = "gui", test))]
    pub fn prev_category(&mut self, engine: &mut EngineMut<'_>) {
        if let Some(section_idx) = self.relative_category_section(engine, -1) {
            self.switch_to_category(engine, section_idx);
        }
    }

    /// delta(±1) 방향으로 순환할 카테고리의 섹션 인덱스를 구한다.
    #[cfg(any(feature = "gui", test))]
    fn relative_category_section(&self, engine: &CoreState, delta: isize) -> Option<usize> {
        if self.active_workspace_index(engine) >= engine.workspaces.len() {
            return None;
        }
        let cat = engine.workspaces[self.active_workspace_index(engine)].category;
        let categories = engine.categories();
        let len = categories.len();
        if len <= 1 {
            return None;
        }
        let pos = categories.iter().position(|c| c.id == cat)?;
        let new_pos = (pos as isize + delta).rem_euclid(len as isize) as usize;
        Some(new_pos)
    }

    /// 순서를 바꾸고 활성 대상을 유지한다. 범위 밖이거나 같은 위치면 false다.
    #[cfg(any(feature = "gui", test))]
    pub fn move_workspace(&mut self, engine: &mut CoreState, from: usize, to: usize) -> bool {
        let len = engine.workspaces.len();
        if from == to || from >= len || to >= len {
            return false;
        }
        let ws = engine.workspaces.remove(from);
        engine.workspaces.insert(to, ws);
        self.reconcile_presentation(engine);
        true
    }

    /// 활성 워크스페이스의 각 pane에서 활성 탭에 속한 지연 터미널을 초기화한다.
    /// 비활성 탭은 전환할 때까지 지연 상태로 남긴다.
    #[cfg(any(feature = "gui", debug_assertions, test))]
    fn ensure_active_workspace_initialized(&mut self, engine: &mut EngineMut<'_>) {
        let mut deferred: Vec<u32> = Vec::new();
        {
            let ws = &engine.workspaces[self.active_workspace_index(engine)];
            for pane_id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id)
                    && let Some(tab) = pane.tabs.get(self.navigation.tab_index(pane))
                {
                    deferred.extend(tab.deferred_surface_ids());
                }
            }
        }
        for surface_id in deferred {
            engine.ensure_surface_initialized(surface_id);
        }
    }

    #[cfg(feature = "gui")]
    pub fn close_active_workspace(&mut self, engine: &mut EngineMut<'_>) -> bool {
        self.close_workspace_at(
            engine,
            self.active_workspace_index(engine),
            WorkspaceCloseOrigin::User,
        )
    }

    /// 지정 워크스페이스를 닫고 관련 상태를 정리한다.
    /// origin은 복원 사본·surface.closed reason·계측 구분을 정한다.
    /// 제거 후 활성 인덱스를 보정하며 workspace.closed는 after_workspace_removed에서 보낸다.
    pub fn close_workspace_at(
        &mut self,
        engine: &mut EngineMut<'_>,
        ws_idx: usize,
        origin: WorkspaceCloseOrigin,
    ) -> bool {
        use crate::close_trace;
        use std::time::Instant;

        let save_snapshot = origin.saves_snapshot();
        let path = origin.trace_path();

        if ws_idx >= engine.workspaces.len() {
            return false;
        }
        // IPC 사전 검사와 별개로 GUI·debug 경로도 hard 점유 조건을 검사한다.
        let in_ws = engine.workspaces[ws_idx].all_surface_ids();
        if self.refuse_if_hard_occupied(engine, in_ws) {
            return false;
        }
        let t_close = Instant::now();
        if save_snapshot {
            let t = Instant::now();
            let snapshot = self.capture_workspace_snapshot(&engine.as_ref(), ws_idx);
            close_trace::log_snapshot(t, &snapshot, path);
            let t = Instant::now();
            engine.push_closed_item(snapshot).log(t.elapsed(), path);
        }
        let t = Instant::now();
        let targets =
            super::RequestContext::collect_workspace_close_targets(&engine.as_ref(), ws_idx);
        close_trace::log_collect(t, targets.len(), path);
        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        self.after_workspace_removed(engine, workspace_id, path);
        self.reconcile_presentation(engine);
        // 제거 후 kind를 찾지 못할 수 있으므로 구독자는 surface ID로도 정리할 수 있어야 한다.
        let zipped: Vec<(u32, Option<String>, Option<&'static str>)> = targets
            .into_iter()
            .map(|(sid, pid)| {
                let kind = self.surface_kind(engine, sid);
                (sid, pid, kind)
            })
            .collect();
        let surfaces = zipped.len();
        self.cleanup_targets(engine, zipped, origin.is_user_close(), Some(path));
        engine.mark_layout_dirty();
        close_trace::log_total(t_close, surfaces, save_snapshot, path);
        true
    }
}

#[cfg(test)]
mod workspace_pointer_tests {

    /// 공통 정리 코드의 보정 호출과 바로 앞 cfg 속성을 검사한다.
    /// 호출 전체를 감싼 GUI 전용 블록까지 판별하지는 못한다.
    #[test]
    fn both_close_cascades_route_through_the_pointer_helper() {
        let src = include_str!("../app/structural_cascade.rs");
        let lines: Vec<&str> = src.lines().collect();
        let calls: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains("state.reconcile_presentation("))
            .map(|(i, _)| i)
            .collect();
        assert!(
            !calls.is_empty(),
            "close cascade 가 활성 포인터 보정 헬퍼를 부르지 않는다"
        );
        for i in calls {
            assert!(
                !lines[i - 1].contains("cfg(feature = \"gui\")"),
                "structural_cascade.rs:{}의 포인터 보정 호출에 gui 전용 조건이 붙어 있다",
                i + 1
            );
        }
        assert!(
            !include_str!("../core/state.rs").contains("pub(crate) restored_active_workspace"),
            "CoreState must not own restored user selection"
        );
    }

    // 인덱스가 우연히 같아지는 경우를 피하도록 대상 ID로 확인한다.
    #[test]
    fn reordering_keeps_the_active_pointer_on_the_same_workspace() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        while engine.workspaces.len() < 4 {
            crate::core::apply_create_workspace_inner(
                &mut engine,
                crate::core::WorkspaceCreationParams::terminal(),
            )
            .unwrap();
        }
        let ids: Vec<u32> = engine.workspaces.iter().map(|w| w.id).collect();

        state.set_active_workspace_index(&engine, 2);
        assert!(state.move_workspace(&mut engine, 0, 3));
        assert_eq!(
            engine.workspaces[state.active_workspace_index(&engine)].id,
            ids[2],
            "앞쪽 워크스페이스가 뒤로 가면 보고 있던 것은 한 칸 당겨진다"
        );

        let from = state.active_workspace_index(&engine);
        assert!(state.move_workspace(&mut engine, from, 0));
        assert_eq!(
            engine.workspaces[state.active_workspace_index(&engine)].id,
            ids[2]
        );
        assert_eq!(state.active_workspace_index(&engine), 0);
    }

    #[test]
    fn the_move_cascade_preserves_the_selected_id() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        for _ in 0..3 {
            crate::core::apply_create_workspace_inner(
                &mut engine,
                crate::core::WorkspaceCreationParams::terminal(),
            )
            .unwrap();
        }
        state.set_active_workspace_index(&engine, 2);
        let selected = state.active_workspace(&engine).id;
        let moved = engine.workspaces.remove(0);
        engine.workspaces.insert(3, moved);
        state.reconcile_presentation(&engine);
        assert_eq!(state.active_workspace(&engine).id, selected);
    }

    #[test]
    fn reordering_keeps_the_category_landing_on_the_same_workspace() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        while engine.workspaces.len() < 4 {
            crate::core::apply_create_workspace_inner(
                &mut engine,
                crate::core::WorkspaceCreationParams::terminal(),
            )
            .unwrap();
        }
        let cat_a = engine.create_category("A").unwrap();
        let cat_b = engine.create_category("B").unwrap();
        let ids: Vec<u32> = engine.workspaces.iter().map(|w| w.id).collect();
        for (i, cat) in [cat_a, cat_b, cat_a, cat_a].into_iter().enumerate() {
            engine.set_workspace_category(ids[i], cat).unwrap();
        }
        assert_eq!(engine.categories()[1].id, cat_a);

        state.switch_workspace(&mut engine, 3);
        state.switch_workspace(&mut engine, 1);

        assert!(state.move_workspace(&mut engine, 0, 3));
        assert_eq!(
            engine.workspaces.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![ids[1], ids[2], ids[3], ids[0]]
        );
        assert_eq!(engine.workspaces_in_category(cat_a)[0].1.id, ids[2]);

        state.switch_to_category(&mut engine, 1);
        assert_eq!(
            engine.workspaces[state.active_workspace_index(&engine)].id,
            ids[3],
            "재정렬 뒤에도 카테고리에서 마지막으로 본 워크스페이스를 선택해야 한다"
        );
    }

    #[test]
    fn category_landing_points_survive_a_removal_because_they_hold_ids() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        while engine.workspaces.len() < 3 {
            crate::core::apply_create_workspace_inner(
                &mut engine,
                crate::core::WorkspaceCreationParams::terminal(),
            )
            .unwrap();
        }
        let cat_a = engine.create_category("A").unwrap();
        let cat_b = engine.create_category("B").unwrap();
        let ids: Vec<u32> = engine.workspaces.iter().map(|w| w.id).collect();
        engine.set_workspace_category(ids[0], cat_b).unwrap();
        engine.set_workspace_category(ids[1], cat_a).unwrap();
        engine.set_workspace_category(ids[2], cat_a).unwrap();

        state.switch_workspace(&mut engine, 2); // A 의 착지점 = ids[2]
        state.switch_workspace(&mut engine, 0); // B 의 착지점 = ids[0]

        engine.workspaces.remove(0);
        state.reconcile_presentation(&engine);

        state.switch_to_category(&mut engine, 1);
        assert_eq!(
            engine.workspaces[state.active_workspace_index(&engine)].id,
            ids[2],
            "제거로 인덱스가 바뀌어도 같은 워크스페이스를 선택해야 한다"
        );

        // 대상과 카테고리의 다른 항목도 없으면 이전 활성 상태를 유지한다.
        let before = state.active_workspace_index(&engine);
        state.switch_to_category(&mut engine, 2);
        assert_eq!(
            state.active_workspace_index(&engine),
            before,
            "빈 카테고리의 이전 선택 기록으로 전환하면 안 된다"
        );
    }
}
