//! 다른 프로그램이 바꾼 폴더를 찾는 주기 확인. 입력을 받는 창(OS 포커스)의 포커스 탭에 있는
//! 로컬 탐색기만 `EXTERNAL_POLL_INTERVAL` 마다 현재 폴더와 펼친 트리 폴더의 표지를 읽고, 표지가 바뀐 폴더만
//! 다시 읽는다. 한 회차는 `MAX_POLL_TARGETS` 개까지만 읽는다. 현재 폴더를 먼저 넣고, 나머지는 가장 오래
//! 확인하지 않은 트리 폴더부터 채워 모든 폴더가 몇 회차 안에 차례로 확인된다. 설정
//! `explorer_refresh_file_details` 를 켜면 같은 회차에 현재 폴더 항목의 metadata 도 비교한다.
//! 현재 폴더는 보이던 목록을 둔 채 다시 읽어 선택·스크롤·포커스를 바꾸지 않는다.
//! 원격 mirror 는 원격 IO 비용 때문에 확인하지 않는다. 규칙: docs/surfaces/explorer/index.md#외부-변경-확인.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{ExplorerView, ExplorerViewStore};
use crate::app::local_reads::{DirStamp, DirStamps, Query, ReadRequests, StampCheck};

/// 확인 주기. 사용자가 정한 값이며 바꾸면 문서와 사이트 가이드도 함께 바꾼다.
pub(crate) const EXTERNAL_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// 한 회차에 표지를 읽는 폴더 수의 상한. 한 회차는 읽기 worker(동시 4개) 하나에서 차례로 읽으므로,
/// 느린 매체에서 폴더 하나에 수십 ms 가 걸려도 한 회차가 주기 안에 끝나고 나머지 worker 는
/// 목록·썸네일·속성 읽기에 남는다. 넘는 트리 폴더는 다음 회차로 넘긴다.
pub(crate) const MAX_POLL_TARGETS: usize = 32;

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
    /// 트리 폴더를 마지막으로 읽거나 확인 대상에 넣은 차례 번호. 작을수록 먼저 확인한다. 같은 회차에
    /// 넣은 폴더도 넣은 순서대로 번호가 달라 줄(FIFO)처럼 돈다. 회차 번호로 매기면 같은 회차 폴더끼리
    /// 경로 순으로 다시 앞에 서서 경로가 앞선 폴더만 매 회차 확인된다.
    checked_seq: HashMap<PathBuf, u64>,
    /// 마지막으로 매긴 차례 번호.
    seq: u64,
    /// 현재 폴더 항목의 metadata 까지 비교하는가(설정 `explorer_refresh_file_details`). 그릴 때 맞춘다.
    file_details: bool,
    /// 현재 폴더 목록을 마지막으로 읽거나 확인했을 때의 항목 서명.
    entries_baseline: Option<(PathBuf, u64)>,
    check: Option<Query<StampCheck>>,
}

impl ExternalPoll {
    /// 이번 회차에 읽을 폴더. 현재 폴더는 늘 넣고, 남은 자리는 가장 오래 확인하지 않은 트리 폴더부터
    /// (확인한 적 없는 폴더끼리는 경로 순) 채운다. 넣은 폴더는 넣은 순서대로 줄의 맨 뒤로 간다.
    fn pick_targets(&mut self, current: Option<&Path>, candidates: &[PathBuf]) -> Vec<PathBuf> {
        let mut targets: Vec<PathBuf> = current
            .filter(|dir| candidates.iter().any(|c| c == dir))
            .map(Path::to_path_buf)
            .into_iter()
            .collect();
        let mut rest: Vec<(u64, &PathBuf)> = candidates
            .iter()
            .filter(|dir| Some(dir.as_path()) != current)
            .map(|dir| (self.checked_seq.get(dir).copied().unwrap_or(0), dir))
            .collect();
        rest.sort_unstable();
        let room = MAX_POLL_TARGETS.saturating_sub(targets.len());
        targets.extend(rest.into_iter().take(room).map(|(_, dir)| dir.clone()));
        for dir in &targets {
            self.mark_checked(dir);
        }
        targets
    }

    pub(super) fn note_read(&mut self, dir: &Path, stamp: Option<DirStamp>) {
        self.baseline.insert(dir.to_path_buf(), stamp);
        // 방금 읽은 폴더는 확인한 것과 같으므로 줄의 맨 뒤로 보낸다.
        self.mark_checked(dir);
    }

    fn mark_checked(&mut self, dir: &Path) {
        self.seq += 1;
        self.checked_seq.insert(dir.to_path_buf(), self.seq);
    }

    /// 현재 폴더 목록을 읽었을 때의 항목 서명. 설정을 나중에 켜도 이 값과 비교한다.
    pub(super) fn note_entries(&mut self, dir: &Path, key: u64) {
        self.entries_baseline = Some((dir.to_path_buf(), key));
    }
}

impl ExplorerView {
    /// 그릴 때마다 부른다. `focused_tab` 은 [`checks_outside_changes`] 의 판정이다.
    /// 포커스를 새로 얻은 프레임에는 바로 확인한다. 주기 확인은 그리지 않고 [`Self::start_due_check`] 가 한다.
    pub(crate) fn poll_external(&mut self, focused_tab: bool, now: Instant) {
        let focused = focused_tab && self.mirror_ws_id.is_none();
        let poll = &mut self.poll;
        poll.focused = focused;
        if !focused {
            poll.next_at = None;
            return;
        }
        if !poll.was_focused || poll.next_at.is_none() {
            self.start_check(now);
        }
    }

    /// 설정 `explorer_refresh_file_details` 를 그릴 때마다 맞춘다. 다음 확인부터 적용한다.
    pub(crate) fn set_file_details_check(&mut self, on: bool) {
        self.poll.file_details = on;
    }

    /// App 타이머가 그리지 않고 부른다. 마지막 프레임에 포커스 탭으로 그려졌고 주기가 됐으면
    /// 확인을 시작한다. 시작했으면 true.
    pub(crate) fn start_due_check(&mut self, now: Instant) -> bool {
        self.poll.focused && self.poll.next_at.is_some_and(|at| now >= at) && self.start_check(now)
    }

    fn start_check(&mut self, now: Instant) -> bool {
        self.poll.next_at = Some(now + EXTERNAL_POLL_INTERVAL);
        // 읽는 중인 폴더는 그 결과가 새 표지를 남기므로 이번 확인은 건너뛴다.
        if self.poll.check.is_some() || self.local_query.is_some() || !self.tree_queries.is_empty()
        {
            return false;
        }
        let candidates = self.poll_candidates();
        let poll = &mut self.poll;
        // 이번 회차에 넣지 못한 폴더도 기준 표지는 남겨야 다음 회차에 변경을 찾는다.
        poll.baseline.retain(|dir, _| candidates.contains(dir));
        poll.checked_seq.retain(|dir, _| candidates.contains(dir));
        if candidates.is_empty() {
            return false;
        }
        let dirs = poll.pick_targets(
            self.loaded.as_ref().map(|(root, _, _)| root.as_path()),
            &candidates,
        );
        let entries_of = poll
            .file_details
            .then(|| self.loaded.as_ref().map(|(root, _, _)| root.clone()))
            .flatten();
        poll.check = Some(crate::app::local_reads::dir_stamps(dirs, entries_of));
        true
    }

    /// 확인할 수 있는 모든 폴더: 현재 폴더와, 하위 목록을 읽어 둔 펼친 트리 폴더.
    fn poll_candidates(&self) -> Vec<PathBuf> {
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

    /// 확인 결과를 반영한다. 다시 읽기만 걸고 화면은 바꾸지 않으므로, 바뀐 것이 없으면
    /// 프레임도 그리지 않는다. 다시 읽은 결과가 오면 그때 그린다.
    pub(super) fn poll_external_result(&mut self, owner: &mut ReadRequests) {
        let Some(result) = self.poll.check.as_mut().and_then(|q| q.poll(owner)) else {
            return;
        };
        self.poll.check = None;
        match result {
            Ok(StampCheck { stamps, entries }) => {
                self.apply_stamps(stamps);
                if let Some((dir, Some(key))) = entries {
                    self.apply_entries_key(dir, key);
                }
            }
            Err(error) => tracing::debug!(%error, "Explorer change check failed"),
        }
    }

    /// 현재 폴더 항목의 metadata 서명이 바뀌었으면 보이던 목록을 둔 채 다시 읽는다. 다시 읽은 목록의
    /// content key 가 바뀐 항목은 Detail 열·미리보기·썸네일이 새 값으로 바뀐다.
    fn apply_entries_key(&mut self, dir: PathBuf, key: u64) {
        if self.loaded_dir() != Some(dir.as_path()) {
            return;
        }
        let changed = self
            .poll
            .entries_baseline
            .as_ref()
            .is_some_and(|(base_dir, base)| *base_dir == dir && *base != key);
        self.poll.entries_baseline = Some((dir.clone(), key));
        if changed && self.local_query.is_none() {
            self.local_query = Some(crate::app::local_reads::stamped_directory(dir));
        }
    }

    /// 표지가 바뀐 폴더만 다시 읽는다. 처음 보는 폴더는 기준으로만 남긴다.
    fn apply_stamps(&mut self, stamps: DirStamps) {
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
                // 보이던 목록을 둔 채 다시 읽고 결과로 바꾼다. 목록 결과가 트리에 캐시된 이 폴더의
                // 하위 목록도 바꾸므로 한 번만 읽는다.
                if self.local_query.is_none() {
                    self.local_query = Some(crate::app::local_reads::stamped_directory(dir));
                }
            } else if self.tree_children.contains_key(&dir) && !self.tree_queries.contains_key(&dir)
            {
                // 보이던 하위 목록을 둔 채 다시 읽어 결과가 오면 바꾼다. 먼저 비우면 사이드바 높이가
                // 줄어 트리 스크롤이 맨 위로 돌아간다.
                let query = crate::app::local_reads::stamped_directory(dir.clone());
                self.tree_queries.insert(dir, query);
            }
        }
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

    /// 주기가 된 포커스 칸의 확인을 그리지 않고 시작한다. 시작한 칸이 있으면 true.
    pub(crate) fn start_due_checks(&mut self, now: Instant) -> bool {
        let mut started = false;
        for view in self.views.values_mut() {
            started |= view.start_due_check(now);
        }
        started
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
