//! 닫힌 surface와 workspace의 도메인 자원을 회수한다.
//! 창 상태 없이 호출할 수 있어 GUI와 headless의 모든 닫기 경로가 같은 정리를 쓴다.

use super::CoreState;
use crate::runtime::engine_access::EngineMut;

impl EngineMut<'_> {
    /// 닫힌 surface의 busy·cwd·mesh frame(mirror 전용)과 attention 레코드를 지운다.
    /// attention은 로컬 surface도 두 빌드에서 가지므로 로컬 레코드도 함께 지운다.
    /// client 쪽 정리는 workspace가 이미 없으면 건너뛰므로 사용자 닫기 경로도 여기서 회수한다.
    fn forget_mirror_surface_extras(&mut self, surface_id: u32) {
        self.remote.mirror_busy_surfaces.remove(&surface_id);
        self.remote.mirror_surface_cwd.remove(&surface_id);
        self.forget_mirror_surface_attention(surface_id);
        #[cfg(feature = "gui")]
        self.remote.attach_mesh_frames.remove(surface_id);
    }

    fn delete_scrollback_persist(persist_id: Option<String>) {
        if let Some(pid) = persist_id {
            crate::scrollback_store::delete(&pid);
        }
    }

    /// surface 범위의 regular·secret 메모리를 삭제한다.
    /// 메타데이터뿐 아니라 플러그인·Lua가 직접 저장한 키도 포함한다.
    fn purge_surface_memory_scope(&self, surface_id: u32) {
        let scope = tasty_memory::Scope::Surface(surface_id);
        match self.with_memory(|m| m.purge_scope(&scope)) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                surface_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-surface scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(surface_id, "memory: purge_scope failed: {e}"),
        }
    }

    /// 제거된 workspace 범위의 regular·secret 메모리를 정리한다.
    /// workspace.closed 통지와 별개이며 통지를 쌓지 않는 headless 연관 제거도 호출한다.
    /// path는 종료 시간 로그의 경로 구분값이다.
    pub(crate) fn purge_workspace_memory_scope(&self, workspace_id: u32, path: &'static str) {
        let t = std::time::Instant::now();
        let ws_scope = tasty_memory::Scope::Workspace(workspace_id);
        match self.with_memory(|m| m.purge_scope(&ws_scope)) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                workspace_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-workspace scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(workspace_id, "memory: purge_scope failed: {e}"),
        }
        crate::close_trace::log_ws_purge(t, path);
    }

    /// 메모리 저장소를 잠그고 함수를 실행한다. poison은 RequestContext와 같은 정책으로 복구한다.
    fn with_memory<R>(&self, f: impl FnOnce(&mut dyn tasty_memory::MemoryStorage) -> R) -> R {
        let mut guard = crate::poison::recover_mutex(
            self.runtime.memory.lock(),
            crate::core::MEMORY_WHAT,
            &crate::core::MEMORY_POISONED,
        );
        f(&mut *guard)
    }
}

impl EngineMut<'_> {
    /// 닫힌 surface의 터미널·인덱스·메모리·점유를 정리하고 단계별 시간을 sums에 합산한다.
    /// persist_id가 있으면 해당 스크롤백 파일 삭제도 시도한다.
    pub(crate) fn cleanup_surface_traced(
        &mut self,
        surface_id: u32,
        persist_id: Option<String>,
        sums: &mut crate::close_trace::CleanupSums,
    ) {
        use std::time::Instant;
        sums.surfaces += 1;
        let t = Instant::now();
        CoreState::delete_scrollback_persist(persist_id);
        sums.scrollback_delete += t.elapsed();
        let t = Instant::now();
        self.drop_terminal(surface_id);
        sums.terminal_drop += t.elapsed();
        let t = Instant::now();
        self.drop_surface_indices(surface_id);
        sums.indices_drop += t.elapsed();
        let t = Instant::now();
        self.purge_surface_memory_scope(surface_id);
        sums.memory_purge += t.elapsed();
        // 닫힌 surface 점유만 지운다. 다른 surface가 남은 workspace 점유는 유지한다.
        self.forget_closed_surface(surface_id);
        self.forget_mirror_surface_extras(surface_id);
    }

    /// Terminal과 부속 상태를 저장소에서 제거한다. 실제 종료 처리는 Terminal의 Drop에 맡긴다.
    fn drop_terminal(&mut self, surface_id: u32) {
        self.pending_scrollback_inject.remove(&surface_id);
        if let Some(old_terminal) = self.runtime.terminals.remove(surface_id) {
            drop(old_terminal);
        }
    }

    fn drop_surface_indices(&mut self, surface_id: u32) {
        self.live.command_index.drop_surface(surface_id);
        self.observer_router.drop_surface(surface_id);
        self.hooks.forget_surface(surface_id);
        self.forget_shell_integration_hint(surface_id);
        if let Some(factory) = self.runtime.waker_factory.as_ref() {
            factory.forget_surface(surface_id);
        }
    }
}

#[cfg(test)]
mod tests {
    /// 사용자가 mirror workspace를 닫으면 attach client가 채운 부속 맵도 비워야 한다.
    /// attach client의 정리는 workspace를 찾지 못하면 건너뛰므로 닫기 정리가 맡는다.
    #[test]
    fn user_close_of_a_mirror_workspace_forgets_the_mirror_maps() {
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let event = crate::app::services::apply_create_workspace_inner(
            &mut engine,
            crate::app::services::WorkspaceCreationParams::terminal(),
        )
        .expect("workspace 생성");
        let crate::app::command::CoreEvent::WorkspaceCreated { index, .. } = event else {
            panic!("expected WorkspaceCreated");
        };
        engine.make_mirror_fixture(index);
        let sid = engine
            .workspace_at(index)
            .expect("workspace index is valid")
            .all_surface_ids()
            .first()
            .copied()
            .expect("surface");

        engine.set_mirror_surface_busy(sid, true);
        engine.set_mirror_surface_cwd(sid, Some("/srv/remote".to_string()));
        engine.set_mirror_surface_attention(sid, Some(crate::core::AttentionKind::NeedsInput));
        #[cfg(feature = "gui")]
        engine
            .remote.attach_mesh_frames
            .update(sid, vec![1, 2, 3], 1, 1, true);

        assert!(state.close_workspace_at(
            &mut engine,
            index,
            crate::state::WorkspaceCloseOrigin::User
        ));

        assert!(!engine.remote.mirror_busy_surfaces.contains(&sid), "busy 남음");
        assert!(!engine.remote.mirror_surface_cwd.contains_key(&sid), "cwd 남음");
        assert!(engine.attention_kind(sid).is_none(), "attention 남음");
        #[cfg(feature = "gui")]
        assert!(
            engine.remote.attach_mesh_frames.get(sid).is_none(),
            "mesh frame 남음"
        );
    }
}
