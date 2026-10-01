//! 닫힌 surface와 workspace의 도메인 자원을 회수한다.
//! 창 상태 없이 호출할 수 있어 GUI와 headless의 모든 닫기 경로가 같은 정리를 쓴다.

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

    /// Execution-owned scope deletion. Journal cleanup propagates failure to its durable result;
    /// the legacy adapter below logs the same failure without inventing a second purge policy.
    pub(crate) fn purge_closed_memory_scope(&self, scope: &tasty_memory::Scope) -> Result<(), String> {
        self.with_memory(|memory| memory.purge_scope(scope))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn purge_surface_memory_scope(&self, surface_id: u32) {
        if let Err(error) = self.purge_closed_memory_scope(&tasty_memory::Scope::Surface(surface_id)) {
            tracing::warn!(surface_id, "memory: purge_scope failed: {error}");
        }
    }

    /// 제거된 workspace 범위의 regular·secret 메모리를 정리한다.
    /// workspace.closed 통지와 별개이며 통지를 쌓지 않는 headless 연관 제거도 호출한다.
    /// path는 종료 시간 로그의 경로 구분값이다.
    pub(crate) fn purge_workspace_memory_scope(&self, workspace_id: u32, path: &'static str) {
        let t = std::time::Instant::now();
        if let Err(error) = self.purge_closed_memory_scope(&tasty_memory::Scope::Workspace(workspace_id)) {
            tracing::warn!(workspace_id, "memory: purge_scope failed: {error}");
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
        Self::delete_scrollback_persist(persist_id);
        sums.scrollback_delete += t.elapsed();
        let t = Instant::now();
        if let Some(owner)=self.runtime.surfaces.remove(&surface_id)
            && let Some(remote)=owner.as_any().downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>() {
            self.runtime.pending_plugin_retirements.push((surface_id,remote.handles().binding()));
        }
        self.drop_terminal(surface_id);
        sums.terminal_drop += t.elapsed();
        let t = Instant::now();
        self.cleanup_surface_observations(surface_id);
        sums.indices_drop += t.elapsed();
        let t = Instant::now();
        self.purge_surface_memory_scope(surface_id);
        sums.memory_purge += t.elapsed();
    }

    /// Detach the content/Pty pair. Its Pty owner performs termination and reaping.
    fn drop_terminal(&mut self, surface_id: u32) {
        self.runtime.pending_scrollback_inject.remove(&surface_id);
        if let Some(old_terminal) = self.runtime.terminals.remove(surface_id) {
            drop(old_terminal);
        }
    }

    /// Non-View teardown shared by committed retirement and legacy execution adapters.
    /// Call only for the captured old owner, while publication excludes replacement installation.
    pub(crate) fn cleanup_surface_observations(&mut self, surface_id: u32) {
        self.runtime.pending_scrollback_inject.remove(&surface_id);
        self.remote.pending_workspace_taps.remove(&surface_id);
        if self.runtime.child_terminals.unregister_child_by_surface(surface_id) {self.runtime.child_terminals.save();}
        #[cfg(feature = "gui")]
        self.runtime.readonly_views.remove(&surface_id);
        self.live.surface_titles.remove(&surface_id);
        self.live.last_key_input.remove(&surface_id);
        self.live.busy_surfaces.remove(&surface_id);
        self.live.mouse_capture_disabled_surfaces.remove(&surface_id);
        self.live.foreground_names.remove(&surface_id);
        self.live.foreground_generation.remove(&surface_id);
        self.live.surface_messages.remove(&surface_id);
        self.forget_closed_surface(surface_id);
        self.forget_mirror_surface_extras(surface_id);
        self.remote.forget_surface_observations(surface_id);
        self.live.command_index.drop_surface(surface_id);
        self.observer_router.drop_surface(surface_id);
        self.hooks.forget_surface(surface_id);
        self.forget_shell_integration_hint(surface_id);
        if let Some(factory) = self.runtime.waker_factory.as_ref() {
            factory.forget_surface(surface_id);
        }
    }
}

