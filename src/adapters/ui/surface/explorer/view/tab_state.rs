//! 내부 탭별 세션 상태. 한 surface 의 `ExplorerView` 는 활성 내부 탭의 목록·선택·Find·트리·읽기를
//! 필드로 들고, 다른 탭의 것은 탭 ID(`ExplorerTabId`)별로 보관한다. 탭을 바꾸면 둘을 맞바꾸므로 돌아온
//! 탭은 보관한 목록과 선택을 바로 보이고 `Loading` 을 거치지 않는다. 같은 폴더를 연 두 탭도 따로 둔다.
//! 활성이 아닌 탭은 이미 받은 읽기·검색 결과만 거두고 새 외부 변경 확인은 시작하지 않는다. 닫힌 탭의
//! 상태는 버린다. 재시작 때는 저장하지 않는다. 규칙: docs/surfaces/explorer/index.md#내부-탭별-상태.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use tasty_model::{ExplorerPanel, ExplorerTabId, SortColumn, SortDir};

use super::{DirEntryInfo, ExplorerView, LoadState, StampedListing, cursor, poll};
use crate::app::local_reads::{Query, ReadRequests};

/// 활성이 아닌 동안 보관하는 탭 하나의 상태.
pub(crate) struct TabSession {
    entries: super::EntrySnapshot,
    entries_gen: u64,
    path_index: std::cell::RefCell<super::path_index::PathIndex>,
    remote_state: HashMap<PathBuf, super::RemoteLoadState>,
    /// 숨김 파일을 끈 동안 목록에서 뺀 항목과, 그렇게 나눌 때의 표시 여부.
    hidden_stash: Vec<DirEntryInfo>,
    split_show: bool,
    loaded: Option<(PathBuf, SortColumn, SortDir)>,
    local_query: Option<Query<StampedListing>>,
    tree_queries: HashMap<PathBuf, Query<StampedListing>>,
    state: LoadState,
    selected: super::SelectedPaths,
    selection_identity: std::sync::Arc<()>,
    anchor: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    tree_children: HashMap<PathBuf, Vec<DirEntryInfo>>,
    reload_requested: bool,
    refresh_in_place: bool,
    existing_ancestor: Option<PathBuf>,
    create: Option<super::super::create::CreateEdit>,
    create_check: Option<super::super::create::WriteCheck>,
    create_refused: bool,
    writable: Option<(PathBuf, bool)>,
    writable_query: Option<(PathBuf, Query<bool>)>,
    find: Option<super::super::find::FindState>,
    poll: poll::ExternalPoll,
    cursor: cursor::CursorState,
}

impl TabSession {
    /// 처음 여는 탭. 첫 동기화가 지금처럼 그 폴더를 읽는다.
    fn fresh(show_hidden: bool) -> Self {
        Self {
            entries: Default::default(),
            entries_gen: 0,
            path_index: super::path_index::empty(),
            remote_state: HashMap::new(),
            hidden_stash: Vec::new(),
            split_show: show_hidden,
            loaded: None,
            local_query: None,
            tree_queries: HashMap::new(),
            state: LoadState::Ok,
            selected: super::SelectedPaths::default(),
            selection_identity: std::sync::Arc::new(()),
            anchor: None,
            expanded: HashSet::new(),
            tree_children: HashMap::new(),
            reload_requested: false,
            refresh_in_place: false,
            existing_ancestor: None,
            create: None,
            create_check: None,
            create_refused: false,
            writable: None,
            writable_query: None,
            find: None,
            poll: Default::default(),
            cursor: Default::default(),
        }
    }

    /// 거둘 읽기나 검색이 있다.
    fn has_pending(&self) -> bool {
        self.local_query.is_some()
            || self.writable_query.is_some()
            || !self.tree_queries.is_empty()
            || self.create_check.is_some()
            || self.find.as_ref().is_some_and(|find| find.has_pending())
    }
}

impl ExplorerView {
    /// 활성 내부 탭이 바뀌었으면 지금 상태를 이전 탭 몫으로 보관하고 새 탭의 상태를 꺼낸다. 처음 여는
    /// 탭은 빈 상태로 시작한다. 닫힌 탭의 상태는 버린다. 폴더 비교(`sync`)보다 먼저 부른다.
    pub(super) fn switch_tab(&mut self, panel: &ExplorerPanel) {
        let live: HashSet<ExplorerTabId> = panel.tabs.iter().map(|t| t.id()).collect();
        self.parked.retain(|id, _| live.contains(id));
        let active = panel.active_tab().id();
        let Some(current) = self.tab.replace(active) else {
            return;
        };
        if current == active {
            return;
        }
        let incoming = self
            .parked
            .remove(&active)
            .unwrap_or_else(|| TabSession::fresh(self.hidden.show));
        let mut outgoing = self.swap_session(incoming);
        // 떠나는 탭의 진행 중 외부 변경 확인과 그 프레임의 포커스는 버린다. 돌아오면 포커스를 되찾은
        // 프레임처럼 바로 다시 확인한다.
        outgoing.poll = outgoing.poll.parked();
        outgoing.cursor = outgoing.cursor.parked();
        outgoing.create = None;
        outgoing.create_check = None;
        outgoing.create_refused = false;
        if let Some(find) = outgoing.find.as_mut() {
            find.leave_tab();
        }
        if live.contains(&current) {
            self.parked.insert(current, outgoing);
        }
        // 입력과 기다리던 자동 선택은 떠난 탭의 작업 맥락이라 이어 가지 않는다.
        self.reveal = None;
        self.reveal_select = false;
        self.reveal_in = None;
        self.addr_probe = None;
        self.cancel_addr_edit();
        self.focus_address = false;
        self.marquee = Default::default();
        self.ops.drag = Default::default();
        self.list_rect = None;
        self.list_layout = None;
        self.scroll_to = None;
        self.type_ahead.reset();
        if let Some(find) = self.find.as_mut() {
            // 입력 포커스는 egui 가 지난 탭의 입력 칸에 두었다. 다음 프레임의 바가 다시 알린다.
            find.field_focused = false;
        }
        // 목록·Find·인덱스를 함께 교환했으므로 저장된 세대와 캐시도 그대로 유효하다.
    }

    /// 지금 탭의 상태를 `session` 으로 바꾸고 이전 상태를 돌려준다. 보관하던 동안 숨김 표시가 바뀌었으면
    /// 그 목록을 지금 표시에 맞게 다시 나눈다.
    fn swap_session(&mut self, mut session: TabSession) -> TabSession {
        let show = self.hidden.show;
        self.exchange(&mut session);
        // 맞바꾼 뒤 `hidden.show` 는 들어온 목록을 나눈 값이다. surface 의 표시와 다르면 다시 나눈다.
        self.set_show_hidden(show);
        session
    }

    /// 필드를 그대로 맞바꾼다. 숨김 표시는 그 목록을 나눈 값으로 맞춘다.
    fn exchange(&mut self, s: &mut TabSession) {
        use std::mem::swap;
        swap(&mut self.entries, &mut s.entries);
        swap(&mut self.entries_gen, &mut s.entries_gen);
        swap(&mut self.path_index, &mut s.path_index);
        swap(&mut self.remote_state, &mut s.remote_state);
        self.hidden.swap_stash(&mut s.hidden_stash);
        swap(&mut self.hidden.show, &mut s.split_show);
        swap(&mut self.loaded, &mut s.loaded);
        swap(&mut self.local_query, &mut s.local_query);
        swap(&mut self.tree_queries, &mut s.tree_queries);
        swap(&mut self.state, &mut s.state);
        swap(&mut self.selected, &mut s.selected);
        swap(&mut self.selection_identity, &mut s.selection_identity);
        swap(&mut self.anchor, &mut s.anchor);
        swap(&mut self.expanded, &mut s.expanded);
        swap(&mut self.tree_children, &mut s.tree_children);
        swap(&mut self.reload_requested, &mut s.reload_requested);
        swap(&mut self.refresh_in_place, &mut s.refresh_in_place);
        swap(&mut self.existing_ancestor, &mut s.existing_ancestor);
        swap(&mut self.create, &mut s.create);
        swap(&mut self.create_check, &mut s.create_check);
        swap(&mut self.create_refused, &mut s.create_refused);
        swap(&mut self.writable, &mut s.writable);
        swap(&mut self.writable_query, &mut s.writable_query);
        swap(&mut self.find, &mut s.find);
        swap(&mut self.poll, &mut s.poll);
        swap(&mut self.cursor, &mut s.cursor);
    }

    /// 보관한 탭마다 `f` 를 그 탭이 활성인 것처럼 부른다. 그리지 않는 갱신(이미 받은 결과, 파일 작업 뒤
    /// 다시 읽기 표시)에만 쓴다.
    pub(super) fn for_each_parked(&mut self, mut f: impl FnMut(&mut ExplorerView)) {
        let ids: Vec<ExplorerTabId> = self.parked.keys().copied().collect();
        for id in ids {
            let Some(mut session) = self.parked.remove(&id) else {
                continue;
            };
            self.exchange(&mut session);
            let active = self.tab.replace(id);
            f(self);
            self.tab = active;
            self.exchange(&mut session);
            self.parked.insert(id, session);
        }
    }

    /// 활성 탭과 보관한 탭 모두에 `f` 를 부른다.
    pub(crate) fn for_each_tab(&mut self, mut f: impl FnMut(&mut ExplorerView)) {
        f(self);
        self.for_each_parked(f);
    }

    /// 보관한 탭이 이미 접수한 목록·트리·쓰기 확인·검색 결과를 그 탭에 거둔다. 화면은 활성 탭만
    /// 그리므로 다시 그리지 않는다.
    pub(super) fn poll_parked_reads(&mut self, owner: &mut ReadRequests) {
        if !self.parked.values().any(TabSession::has_pending) {
            return;
        }
        self.for_each_parked(|view| {
            view.poll_tab_reads(owner);
        });
    }

    pub(super) fn pending_dir_any_tab(&self, request_id: u64) -> Option<PathBuf> {
        self.find_pending_dir(request_id).or_else(|| {
            self.parked.values().find_map(|session| {
                session
                    .remote_state
                    .iter()
                    .find_map(|(dir, state)| match state {
                        super::RemoteLoadState::Loading { request_id: id, .. }
                            if *id == request_id =>
                        {
                            Some(dir.clone())
                        }
                        _ => None,
                    })
            })
        })
    }

    pub(crate) fn tab_id(&self) -> Option<ExplorerTabId> {
        self.tab
    }

    /// 스크롤·입력 같은 egui 위젯 ID 에 섞을 활성 내부 탭 값. 탭마다 위젯 상태(스크롤 위치)를 따로 두고,
    /// 탭 목록 안 자리가 바뀌어도 같은 탭은 같은 ID 를 쓴다.
    pub(crate) fn tab_salt(&self) -> u64 {
        self.tab.map_or(0, ExplorerTabId::get)
    }

    /// 탭 상태 보관 수. 시험과 진단에서 닫힌 탭이 정리됐는지 본다.
    #[cfg(test)]
    pub(crate) fn parked_tab_count(&self) -> usize {
        self.parked.len()
    }
}

#[cfg(test)]
#[path = "tab_state_tests.rs"]
mod tests;
