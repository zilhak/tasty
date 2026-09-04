use crate::core::CoreState;

use super::AppState;

/// 워크스페이스 하나가 `removed_idx` 에서 제거된 뒤, **인덱스로 저장된 활성 포인터**가
/// 계속 같은 워크스페이스를 가리키도록 보정한 값.
///
/// `active_workspace` 는 인덱스가 진실 소스라, 앞쪽 워크스페이스가 빠지면 뒤 워크스페이스
/// 들이 한 칸씩 당겨지면서 손대지 않은 인덱스가 **다른 워크스페이스**를 가리키게 된다.
/// 사용자가 보고 있던 것을 닫은 경우(`active == removed_idx`)에만 시야가 움직이고, 그때는
/// 그 자리로 밀려 들어온 워크스페이스(마지막이었다면 직전 것)로 착지한다.
/// 근거: [`docs/design/policies/focus.md`] "삭제로 인한 인덱스 이동".
///
/// `remaining` 은 제거 **후** 남은 워크스페이스 수다. 0 이면 호출자가 곧 workspace 를
/// 자동 재생성하므로(빈 화면 invariant) 0 을 돌려준다.
pub(crate) fn active_index_after_removal(
    active: usize,
    removed_idx: usize,
    remaining: usize,
) -> usize {
    if remaining == 0 {
        return 0;
    }
    if active > removed_idx {
        active - 1
    } else if active == removed_idx {
        active.min(remaining - 1)
    } else {
        active
    }
}

impl AppState {
    /// 워크스페이스 제거 직후, 인덱스를 값으로 들고 있는 **모든** 활성 포인터를 대상
    /// 기준으로 보정한다 — `active_workspace` 와 카테고리별 last-active 착지점.
    ///
    /// 호출자는 `engine.workspaces.remove(removed_idx)` **직후**에 부른다.
    pub(crate) fn fix_workspace_pointers_after_removal(
        &mut self,
        removed_idx: usize,
        remaining: usize,
    ) {
        self.active_workspace =
            active_index_after_removal(self.active_workspace, removed_idx, remaining);
        // 카테고리 quick-switch 착지점도 같은 밀림을 겪는다. 제거된 워크스페이스를
        // 가리키던 항목은 착지 대상이 사라진 것이라 지운다(사용 시점에 first 로 폴백).
        self.category_last_active
            .retain(|_, idx| *idx != removed_idx);
        for idx in self.category_last_active.values_mut() {
            if *idx > removed_idx {
                *idx -= 1;
            }
        }
    }

    /// Switch to workspace by index (0-based).
    pub fn switch_workspace(&mut self, engine: &mut CoreState, index: usize) {
        if index < engine.workspaces.len() {
            self.active_workspace = index;
            // 카테고리별 last-active 기록 — 카테고리 quick-switch(T4WS ⑤) 착지점.
            let cat = engine.workspaces[index].category;
            self.category_last_active.insert(cat, index);
            self.ensure_active_workspace_initialized(engine);
        }
    }

    /// 섹션 순서 인덱스(0=reserved normal, 1.. = 사용자 카테고리)로 **카테고리 자체**를
    /// 전환한다 (T4WS ②⑤, `Alt+Shift+숫자`). folders 기능 on 에서만 호출된다.
    ///
    /// 동작: (1) 대상 카테고리가 접혀 있으면 **auto-expand**(persist) — 접힌 채면 착지
    /// 워크스페이스가 안 보이므로 펼치는 게 옳다. (2) 그 카테고리의 **last-active**
    /// 워크스페이스로 착지(없거나 stale 이면 **first**). 착지는 전역 인덱스 SoT 를 쓰는
    /// [`switch_workspace`](Self::switch_workspace) 재사용.
    //
    // 사용자 키 경로로만 호출(원칙 1/3: active_workspace 이동은 release IPC/CLI 노출 금지).
    pub fn switch_to_category(&mut self, engine: &mut CoreState, section_idx: usize) {
        let Some(cat) = engine.categories().get(section_idx).map(|c| c.id) else {
            return;
        };
        // (1) auto-expand + persist.
        let collapsed = engine
            .categories()
            .get(section_idx)
            .is_some_and(|c| c.collapsed);
        if collapsed {
            engine.set_category_collapsed(cat, false);
            engine.mark_layout_dirty();
        }
        // (2) last-active(소속 재검증) → 없으면 first-in-category.
        let target = self
            .category_last_active
            .get(&cat)
            .copied()
            .filter(|&gi| engine.workspaces.get(gi).is_some_and(|w| w.category == cat))
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

    /// 카테고리-로컬 인덱스로 전환 (S-WSCAT). 현재 active 워크스페이스가 속한
    /// 카테고리의 로컬 목록에서 `local_idx` 번째를 골라 **전역 인덱스로 변환**한 뒤
    /// 기존 [`switch_workspace`](Self::switch_workspace) 를 재사용한다. 전역 인덱스가
    /// 단일 진실 소스이므로 move/close/cascade 의 active 보정 로직을 그대로 쓴다.
    /// 카테고리 토글 off 거나 active 카테고리에 `local_idx` 가 없으면 no-op.
    pub fn switch_workspace_in_active_category(
        &mut self,
        engine: &mut CoreState,
        local_idx: usize,
    ) {
        if self.active_workspace >= engine.workspaces.len() {
            return;
        }
        let cat = engine.workspaces[self.active_workspace].category;
        let global = engine
            .workspaces_in_category(cat)
            .get(local_idx)
            .map(|(gi, _)| *gi);
        if let Some(global) = global {
            self.switch_workspace(engine, global);
        }
    }

    /// 현재 active 워크스페이스가 속한 카테고리 내에서 **다음** 워크스페이스로 이동한다.
    /// 표시(=저장) 순서를 따르고, 마지막에서 다음으로 가면 `workspace_switch_crosses_category`
    /// 설정에 따라 같은 카테고리의 첫 항목으로 wrap-around 하거나(off, 기본) 다음
    /// 카테고리의 첫 워크스페이스로 넘어간다(on). 카테고리에 자기 자신뿐이면 옵션 off 시
    /// no-op(`Pane::next_tab` 의 `len > 1` 가드와 동형), on 이면 인접 카테고리로 이동.
    /// 전역 인덱스만 뽑아 불변 빌림을 끝낸 뒤 [`switch_workspace`](Self::switch_workspace)
    /// 를 재사용하므로 active 보정 로직을 그대로 탄다.
    //
    // quick-switch 키바인딩(QS03)에서 **사용자 키 경로로만** 호출된다. (원칙 1/3:
    // active_workspace 를 바꾸는 사용자 포커스 이동 — release IPC/CLI 로 노출 금지.)
    pub fn next_workspace_in_active_category(&mut self, engine: &mut CoreState) {
        if let Some(target) = self.relative_workspace_in_active_category(engine, 1) {
            self.switch_workspace(engine, target);
        }
    }

    /// 현재 active 워크스페이스가 속한 카테고리 내에서 **이전** 워크스페이스로 이동한다.
    /// [`next_workspace_in_active_category`](Self::next_workspace_in_active_category) 의
    /// 역방향(첫 항목에서 이전으로 가면 `workspace_switch_crosses_category` off 시 마지막
    /// 항목으로 wrap-around, on 시 이전 카테고리의 마지막 워크스페이스로 이동).
    // QS03 에서 사용자 키 경로로 호출 (next_ 동일).
    pub fn prev_workspace_in_active_category(&mut self, engine: &mut CoreState) {
        if let Some(target) = self.relative_workspace_in_active_category(engine, -1) {
            self.switch_workspace(engine, target);
        }
    }

    /// active 워크스페이스가 속한 카테고리 로컬 목록에서 `delta`(±1) 만큼 이동한 대상의
    /// **전역 인덱스** 를 반환한다. active OOB · 로컬 위치 미검출(방어) 시 `None`.
    /// 반환값은 usize 복사본이라 호출부에서 불변 빌림 없이 가변 `switch_workspace` 를
    /// 호출할 수 있다.
    ///
    /// `workspace_switch_crosses_category` 옵션이 on 이고 이동이 카테고리 경계를 벗어나면
    /// [`relative_category_boundary_workspace`](Self::relative_category_boundary_workspace)
    /// 로 인접 카테고리의 첫/마지막 워크스페이스로 넘어간다(카테고리가 1개뿐이라 넘어갈
    /// 곳이 없으면 아래 로컬 wrap 으로 자연히 폴백). off 이거나 로컬 목록이 1개 이하면
    /// 기존과 동일하게 카테고리 로컬 wrap 만 수행한다.
    fn relative_workspace_in_active_category(
        &self,
        engine: &CoreState,
        delta: isize,
    ) -> Option<usize> {
        if self.active_workspace >= engine.workspaces.len() {
            return None;
        }
        let cat = engine.workspaces[self.active_workspace].category;
        let locals = engine.workspaces_in_category(cat);
        let len = locals.len();
        let pos = locals
            .iter()
            .position(|(gi, _)| *gi == self.active_workspace)?;

        if engine.settings.general.workspace_switch_crosses_category {
            let raw = pos as isize + delta;
            if raw < 0 || raw >= len as isize {
                if let Some(target) = self.relative_category_boundary_workspace(engine, delta) {
                    return Some(target);
                }
                // 인접 카테고리가 없음(카테고리 1개) → 아래 로컬 wrap 으로 폴백.
            } else {
                return Some(locals[raw as usize].0);
            }
        }

        if len <= 1 {
            return None;
        }
        // len - 1 == delta.rem_euclid 을 위한 wrap: (pos + len ± 1) % len.
        let new_pos = (pos as isize + delta).rem_euclid(len as isize) as usize;
        Some(locals[new_pos].0)
    }

    /// `relative_workspace_in_active_category` 가 카테고리 경계를 넘을 때 호출한다.
    /// `delta` 방향의 인접 카테고리로 넘어가 그 카테고리의 **첫**(다음 방향, `delta > 0`)
    /// 또는 **마지막**(이전 방향) 워크스페이스의 전역 인덱스를 반환한다.
    /// [`switch_to_category`](Self::switch_to_category) 의 last-active 착지와 달리
    /// 항상 방향에 맞는 끝 원소로 착지해야 방향성이 유지된다. 카테고리가 1개 이하이면
    /// [`relative_category_section`](Self::relative_category_section) 이 `None` 을
    /// 반환해 호출부가 로컬 wrap 으로 폴백한다.
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

    /// 현재 active 워크스페이스가 속한 카테고리의 **다음** 카테고리로 전환한다(T4WS
    /// 카테고리 축 quick-switch next/prev). `engine.categories()`(0=reserved normal,
    /// 1.. = 사용자 카테고리) 리스트 안에서 현재 카테고리 위치를 찾아 `rem_euclid` 로
    /// wrap-around ±1 이동한 뒤 그 section_idx 로 [`switch_to_category`](Self::switch_to_category)
    /// 를 재사용한다(auto-expand + last-active 착지 포함). 카테고리가 1개 이하면
    /// no-op([`relative_workspace_in_active_category`] 의 `len <= 1` 가드와 동형).
    //
    // quick-switch 키바인딩에서 **사용자 키 경로로만** 호출된다(원칙 1/3).
    pub fn next_category(&mut self, engine: &mut CoreState) {
        if let Some(section_idx) = self.relative_category_section(engine, 1) {
            self.switch_to_category(engine, section_idx);
        }
    }

    /// 현재 active 워크스페이스가 속한 카테고리의 **이전** 카테고리로 전환한다.
    /// [`next_category`](Self::next_category) 의 역방향(wrap-around 포함).
    pub fn prev_category(&mut self, engine: &mut CoreState) {
        if let Some(section_idx) = self.relative_category_section(engine, -1) {
            self.switch_to_category(engine, section_idx);
        }
    }

    /// active 워크스페이스가 속한 카테고리로부터 `delta`(±1) 만큼 wrap-around 이동한
    /// 카테고리의 **section_idx**(= `engine.categories()` 리스트 내 위치)를 반환한다.
    /// active OOB · 카테고리 1개 이하 · 위치 미검출(방어) 시 `None`.
    fn relative_category_section(&self, engine: &CoreState, delta: isize) -> Option<usize> {
        if self.active_workspace >= engine.workspaces.len() {
            return None;
        }
        let cat = engine.workspaces[self.active_workspace].category;
        let categories = engine.categories();
        let len = categories.len();
        if len <= 1 {
            return None;
        }
        let pos = categories.iter().position(|c| c.id == cat)?;
        let new_pos = (pos as isize + delta).rem_euclid(len as isize) as usize;
        Some(new_pos)
    }

    /// Move a workspace from one index to another, adjusting active_workspace accordingly.
    /// Returns false if indices are out of bounds or equal.
    pub fn move_workspace(&mut self, engine: &mut CoreState, from: usize, to: usize) -> bool {
        let len = engine.workspaces.len();
        if from == to || from >= len || to >= len {
            return false;
        }
        let ws = engine.workspaces.remove(from);
        engine.workspaces.insert(to, ws);
        // Adjust active_workspace to follow the moved workspace or account for the shift
        if self.active_workspace == from {
            self.active_workspace = to;
        } else if from < to && self.active_workspace > from && self.active_workspace <= to {
            self.active_workspace -= 1;
        } else if from > to && self.active_workspace >= to && self.active_workspace < from {
            self.active_workspace += 1;
        }
        true
    }

    /// 활성 workspace에서 사용자가 보고 있는 active_tab의 deferred surface(들)만 PTY를
    /// spawn. 같은 pane의 비활성 tab은 deferred로 남았다가 tab 전환 시 깨어난다.
    /// active_tab이 split layout이면 그 안의 모든 deferred placeholder를 한번에 spawn한다.
    fn ensure_active_workspace_initialized(&mut self, engine: &mut CoreState) {
        let mut spawned: Vec<(u32, tasty_terminal::Terminal, Option<String>)> = Vec::new();
        {
            let ws = &mut engine.workspaces[self.active_workspace];
            let pane_ids: Vec<u32> = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    let active_idx = pane.active_tab;
                    if let Some(tab) = pane.tabs.get_mut(active_idx) {
                        let mut entries = tab.ensure_all_initialized();
                        spawned.append(&mut entries);
                    }
                }
            }
        }
        for (surface_id, terminal, persist_id) in spawned {
            engine.terminals.insert(surface_id, terminal);
            if let Some(pid) = persist_id {
                engine.terminals.set_scrollback_persist_id(surface_id, pid);
            }
            engine.send_fast_init(surface_id);
            engine.apply_pending_scrollback_inject(surface_id);
        }
    }

    /// Close the active workspace. Returns true if the workspace was removed.
    /// Cleans up all surfaces (surface meta + per-surface view state) in the workspace.
    pub fn close_active_workspace(&mut self, engine: &mut CoreState) -> bool {
        self.close_workspace_at(engine, self.active_workspace)
    }

    /// Close a specific workspace by index (context menu 등 임의 지정 close).
    /// Cleans up all surfaces + closed_item snapshot + memory scope purge.
    pub fn close_workspace_at(&mut self, engine: &mut CoreState, ws_idx: usize) -> bool {
        use crate::close_trace;
        use std::time::Instant;

        /// close 계측의 경로 구분값 — 이 함수는 GUI 트리거 전용이다.
        const PATH: &str = "gui";

        if ws_idx >= engine.workspaces.len() {
            return false;
        }
        let t_close = Instant::now();
        // C1 — Capture workspace snapshot before closing.
        let t = Instant::now();
        let snapshot = super::AppState::capture_workspace_snapshot(engine, ws_idx);
        close_trace::log_snapshot(t, &snapshot, PATH);
        // C2 — restore.command 주입 + 스크롤백 디스크 write + evict.
        let t = Instant::now();
        engine.push_closed_item(snapshot).log(t.elapsed(), PATH);
        // C3 — Collect all (surface_id, persist_id) for cleanup before removing.
        let t = Instant::now();
        let targets = super::AppState::collect_workspace_close_targets(engine, ws_idx);
        close_trace::log_collect(t, targets.len(), PATH);
        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        // C4 — Workspace scope 의 memory entry 정리. 안의 surface 들은 아래
        // cleanup_surface 에서 각자 자기 scope 를 purge 한다.
        let t = Instant::now();
        self.purge_workspace_memory_scope(workspace_id);
        close_trace::log_ws_purge(t, PATH);
        // 활성 포인터를 대상 기준으로 보정 — 앞쪽 워크스페이스를 닫아도 보고 있던
        // 워크스페이스가 그대로 남는다.
        self.fix_workspace_pointers_after_removal(ws_idx, engine.workspaces.len());
        // Cleanup
        // workspace.remove 후엔 surface_kind 가 None 을 반환할 수 있으나, plugin
        // lifecycle 구독자는 surface_id 만으로 cleanup 가능 (R1 분석).
        let zipped: Vec<(u32, Option<String>, Option<&'static str>)> = targets
            .into_iter()
            .map(|(sid, pid)| {
                let kind = self.surface_kind(engine, sid);
                (sid, pid, kind)
            })
            .collect();
        let surfaces = zipped.len();
        self.cleanup_targets(engine, zipped, true, Some(PATH));
        engine.mark_layout_dirty();
        close_trace::log_total(t_close, surfaces, true, PATH);
        true
    }
}

#[cfg(test)]
mod workspace_pointer_tests {
    //! workspace 제거 후 인덱스 활성 포인터 보정 규칙(`active_index_after_removal`)과
    //! 그 적용(`fix_workspace_pointers_after_removal`)을 고정한다.
    use super::*;

    #[test]
    fn removing_an_earlier_workspace_shifts_the_active_pointer_down() {
        // [0,1,2,3] 에서 사용자는 2 를 보는 중, 0 이 제거됨 → 같은 대상은 이제 1.
        assert_eq!(active_index_after_removal(2, 0, 3), 1);
    }

    #[test]
    fn removing_a_later_workspace_leaves_the_active_pointer_alone() {
        assert_eq!(active_index_after_removal(2, 3, 3), 2);
    }

    #[test]
    fn removing_the_active_workspace_lands_on_the_one_that_slid_in() {
        assert_eq!(active_index_after_removal(1, 1, 3), 1);
    }

    #[test]
    fn removing_the_active_last_workspace_falls_back_to_the_previous_one() {
        assert_eq!(active_index_after_removal(3, 3, 3), 2);
    }

    #[test]
    fn removing_the_only_workspace_yields_zero() {
        // 호출자가 곧 workspace 를 자동 재생성한다(빈 화면 invariant).
        assert_eq!(active_index_after_removal(0, 0, 0), 0);
    }

    /// **두 실행 형태 모두** close cascade 가 이 헬퍼를 지나는지 소스 수준으로 고정한다.
    ///
    /// cascade 는 gui(`app/dispatch_domain.rs`)와 headless(`app/dispatch_domain_stubs.rs`)
    /// 로 `#[cfg(feature = "gui")]` 분기되어 있다. CI 는 이제 headless 도 실행한다
    /// (`.github/workflows/crossplatform-check.yml` 의 `cargo test --workspace --lib
    /// --bins --no-default-features`). **그런데도 이 소스 가드가 여전히 필요하다** —
    /// 이 불변식은 headless **행동** 테스트로 원리적으로 잡히지 않기 때문이다: 오늘의
    /// headless 는 `active_workspace` 가 0 을 벗어날 수단이 없어(레이아웃 복원 없음 ·
    /// `preset.apply` 가 `focus: false` 강제 · `debug.switch_workspace` 는 gui 게이트)
    /// 올바른 보정과 옛 범위 초과 clamp 의 **결과가 같다**. 그래서 headless 쪽만 옛
    /// clamp 로 남아도 어떤 실행 테스트도 실패하지 않는다 — 실제로 그렇게 한 번
    /// 놓쳤고, 그때 잡은 것도 (기본 빌드에서 도는) 이 소스 가드였다.
    /// 근거 [ADR-0113](../../docs/adr/0113-close-preserves-the-focused-target.md).
    #[test]
    fn both_close_cascades_route_through_the_pointer_helper() {
        for (label, src) in [
            ("gui", include_str!("../app/dispatch_domain.rs")),
            ("headless", include_str!("../app/dispatch_domain_stubs.rs")),
        ] {
            assert!(
                src.contains("fix_workspace_pointers_after_removal"),
                "{label} cascade 가 활성 포인터 보정 헬퍼를 부르지 않는다"
            );
            assert!(
                !src.contains("state.active_workspace = engine.workspaces.len() - 1"),
                "{label} cascade 에 범위 초과 clamp 만 하는 옛 보정이 남아 있다"
            );
        }
    }

    #[test]
    fn category_landing_points_follow_the_same_shift() {
        let (mut state, _engine) = crate::state::tests::test_state();
        let cat_a = tasty_utils::id::WorkspaceCategoryId::from(7u32);
        let cat_b = tasty_utils::id::WorkspaceCategoryId::from(8u32);
        state.active_workspace = 2;
        state.category_last_active.insert(cat_a, 3);
        state.category_last_active.insert(cat_b, 0);

        state.fix_workspace_pointers_after_removal(0, 3);

        assert_eq!(state.active_workspace, 1);
        assert_eq!(
            state.category_last_active.get(&cat_a).copied(),
            Some(2),
            "뒤쪽 착지점은 한 칸 당겨져 같은 워크스페이스를 가리켜야 한다"
        );
        assert_eq!(
            state.category_last_active.get(&cat_b),
            None,
            "제거된 워크스페이스를 가리키던 착지점은 지워져 first 로 폴백한다"
        );
    }
}
