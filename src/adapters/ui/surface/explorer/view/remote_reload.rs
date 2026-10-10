//! 원격 목록 캐시의 쓰기용 보조. 원격 작업 뒤 목록을 낡은 것으로 표시하고(로컬 작업의
//! `invalidate_local` 과 같은 규칙), 끌어 놓기가 쓰는 폴더별 디스크 식별값을 기억한다.

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

impl super::ExplorerView {
    /// 받은 원격 목록의 디스크 식별값을 기억한다. 옛 서버처럼 값이 없으면 잊는다.
    pub(super) fn note_remote_volume(&mut self, dir: PathBuf, volume: Option<u64>) {
        match volume {
            Some(volume) => self.remote_volumes.insert(dir, volume),
            None => self.remote_volumes.remove(&dir),
        };
    }

    /// `dir` 의 원격 디스크 식별값. 아직 읽지 않은 폴더는 상위 폴더의 값을 쓴다(그 안의 하위 폴더에
    /// 놓을 때). 모르면 None 이다.
    pub(crate) fn remote_volume_of(&self, dir: &Path) -> Option<u64> {
        self.remote_volumes
            .get(dir)
            .or_else(|| dir.parent().and_then(|p| self.remote_volumes.get(p)))
            .copied()
    }
}
