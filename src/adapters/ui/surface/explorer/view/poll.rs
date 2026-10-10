//! 다른 프로그램이 바꾼 폴더를 찾는 주기 확인. 입력을 받는 창(OS 포커스)의 포커스 탭에 있는
//! 로컬 탐색기만 `EXTERNAL_POLL_INTERVAL` 마다 현재 폴더와 펼친 트리 폴더의 표지를 읽고, 표지가 바뀐 폴더만
//! 다시 읽는다. 현재 폴더는 보이던 목록을 둔 채 다시 읽어 선택·스크롤·포커스를 바꾸지 않는다.
//! 원격 mirror 는 원격 IO 비용 때문에 확인하지 않는다. 규칙: docs/surfaces/explorer/index.md#외부-변경-확인.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{ExplorerView, ExplorerViewStore};
use crate::app::local_reads::{DirStamp, DirStamps, Query, ReadRequests};

/// 확인 주기. 사용자가 정한 값이며 바꾸면 문서와 사이트 가이드도 함께 바꾼다.
pub(crate) const EXTERNAL_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// 이 칸을 확인할지. 입력을 받는 창의 포커스 탭에 속한 칸만 확인하므로 Tasty 가 배경 앱이면
/// 어느 칸도 확인하지 않는다. 창이나 탭이 포커스를 되찾은 프레임에는 바로 확인한다.
pub(crate) fn checks_outside_changes(window_focused: bool, in_focused_tab: bool) -> bool {
    window_focused && in_focused_tab
}

#[derive(Default)]
pub(crate) struct ExternalPoll {
    /// 이번 프레임에 포커스 탭으로 그려졌는가. 프레임 시작에 내린다.
    focused: bool,
    was_focused: bool,
    next_at: Option<Instant>,
    /// 폴더를 마지막으로 읽거나 확인했을 때의 표지.
    baseline: HashMap<PathBuf, Option<DirStamp>>,
    check: Option<Query<DirStamps>>,
    quiet_reload: bool,
}

impl ExternalPoll {
    pub(super) fn note_read(&mut self, dir: &Path, stamp: Option<DirStamp>) {
        self.baseline.insert(dir.to_path_buf(), stamp);
    }

    /// 현재 폴더를 보이던 목록을 둔 채 다시 읽을 차례인가. 읽으면 내린다.
    pub(super) fn take_quiet_reload(&mut self) -> bool {
        std::mem::take(&mut self.quiet_reload)
    }
}

impl ExplorerView {
    /// 그릴 때마다 부른다. `focused_tab` 은 [`checks_outside_changes`] 의 판정이다.
    /// 포커스를 새로 얻은 프레임에는 바로 확인하고, 그 뒤로는 주기마다 확인한다.
    pub(crate) fn poll_external(&mut self, focused_tab: bool, now: Instant) {
        let focused = focused_tab && self.mirror_ws_id.is_none();
        let poll = &mut self.poll;
        poll.focused = focused;
        if !focused {
            poll.next_at = None;
            return;
        }
        let due = !poll.was_focused || poll.next_at.is_none_or(|at| now >= at);
        if !due {
            return;
        }
        poll.next_at = Some(now + EXTERNAL_POLL_INTERVAL);
        // 읽는 중인 폴더는 그 결과가 새 표지를 남기므로 이번 확인은 건너뛴다.
        if poll.check.is_some() || self.local_query.is_some() || !self.tree_queries.is_empty() {
            return;
        }
        let dirs = self.poll_targets();
        let poll = &mut self.poll;
        poll.baseline.retain(|dir, _| dirs.contains(dir));
        if !dirs.is_empty() {
            poll.check = Some(crate::app::local_reads::dir_stamps(dirs));
        }
    }

    /// 현재 폴더와, 하위 목록을 읽어 둔 펼친 트리 폴더.
    fn poll_targets(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self
            .loaded_dir()
            .map(Path::to_path_buf)
            .into_iter()
            .collect();
        for dir in &self.expanded {
            if self.tree_children.contains_key(dir) && !dirs.contains(dir) {
                dirs.push(dir.clone());
            }
        }
        dirs
    }

    /// 확인 결과를 반영한다. 다시 읽을 폴더가 생겼으면 true.
    pub(super) fn poll_external_result(&mut self, owner: &mut ReadRequests) -> bool {
        let Some(result) = self.poll.check.as_mut().and_then(|q| q.poll(owner)) else {
            return false;
        };
        self.poll.check = None;
        match result {
            Ok(stamps) => self.apply_stamps(stamps),
            Err(error) => {
                tracing::debug!(%error, "Explorer change check failed");
                false
            }
        }
    }

    /// 표지가 바뀐 폴더만 다시 읽게 한다. 처음 보는 폴더는 기준으로만 남긴다.
    fn apply_stamps(&mut self, stamps: DirStamps) -> bool {
        let mut any = false;
        for (dir, stamp) in stamps {
            let changed = self
                .poll
                .baseline
                .insert(dir.clone(), stamp)
                .is_some_and(|old| old != stamp);
            if !changed {
                continue;
            }
            if self.loaded_dir() == Some(dir.as_path()) {
                // 목록 결과가 트리에 캐시된 이 폴더의 하위 목록도 바꾸므로 한 번만 읽는다.
                self.poll.quiet_reload = true;
                any = true;
            } else if self.tree_children.contains_key(&dir) && !self.tree_queries.contains_key(&dir)
            {
                // 보이던 하위 목록을 둔 채 다시 읽어 결과가 오면 바꾼다. 먼저 비우면 사이드바 높이가
                // 줄어 트리 스크롤이 맨 위로 돌아간다.
                let query = crate::app::local_reads::stamped_directory(dir.clone());
                self.tree_queries.insert(dir, query);
                any = true;
            }
        }
        any
    }
}

impl ExplorerViewStore {
    /// 창의 패널을 그리기 전에 부른다. 이번 프레임에 그려지지 않은 칸은 확인하지 않는다.
    pub(crate) fn begin_poll_frame(&mut self) {
        for view in self.views.values_mut() {
            view.poll.was_focused = view.poll.focused;
            view.poll.focused = false;
        }
    }

    /// 포커스 탭 탐색기의 가장 이른 다음 확인 시각. 없으면 깨울 필요가 없다.
    pub(crate) fn next_poll_at(&self) -> Option<Instant> {
        self.views
            .values()
            .filter(|view| view.poll.focused)
            .filter_map(|view| view.poll.next_at)
            .min()
    }
}

#[cfg(test)]
#[path = "poll_tests.rs"]
mod tests;
