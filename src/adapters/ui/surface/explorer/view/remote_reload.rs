//! 원격 작업 뒤 원격 목록 캐시를 낡은 것으로 표시한다. 로컬 작업의 `invalidate_local` 과 같은 규칙을
//! 같은 mirror workspace 의 explorer 에 적용한다.

use std::path::{Path, PathBuf};

impl super::ExplorerViewStore {
    /// 원격 작업이 바꾼 폴더를 보거나 기억하는 같은 mirror workspace 의 View 를 다시 읽게 한다.
    /// 응답을 기다리는 폴더는 그대로 둔다. 다시 읽은 View 가 있으면 true.
    pub(crate) fn invalidate_remote(
        &mut self,
        workspace: u32,
        changed: &[PathBuf],
        removed: &[PathBuf],
    ) -> bool {
        let hit = |dir: &Path| {
            changed.iter().any(|c| c == dir) || removed.iter().any(|r| dir.starts_with(r))
        };
        let mut any = false;
        for view in self.views.values_mut() {
            if view.mirror_ws_id != Some(workspace) {
                continue;
            }
            let stale: Vec<PathBuf> = view
                .remote_state
                .iter()
                .filter(|(dir, state)| {
                    !matches!(state, super::RemoteLoadState::Loading { .. }) && hit(dir)
                })
                .map(|(dir, _)| dir.clone())
                .chain(view.tree_children.keys().filter(|dir| hit(dir)).cloned())
                .collect();
            for dir in &stale {
                view.remote_state.remove(dir);
                view.tree_children.remove(dir);
            }
            if view.loaded.as_ref().is_some_and(|(dir, _, _)| hit(dir)) {
                view.request_reload();
                any = true;
            }
            any |= !stale.is_empty();
        }
        any
    }

    /// 원격 쓰기를 받는 mirror workspace 를 바꾼다. 다음 동기화부터 칸의 `writes_allowed` 에 반영한다.
    pub(crate) fn set_remote_writable(&mut self, workspaces: &std::collections::HashSet<u32>) {
        if self.remote_writable != *workspaces {
            self.remote_writable.clone_from(workspaces);
        }
    }
}
