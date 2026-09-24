//! surface별 탐색기 표시 상태. 모델의 탐색 이력과 별도로 목록 캐시·선택·트리 펼침을 보관한다.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tasty_model::{ExplorerPanel, SortColumn, SortDir, SurfaceId};

use super::type_ahead::TypeAhead;
pub(crate) use crate::core::fs_list::{DirEntryInfo, human_size};
use crate::core::fs_list::{read_dir_entries, sort_entries};
use crate::i18n::t;

/// 디렉토리 로드 결과 상태 (content 중앙 상태 텍스트로 표현 — design §3.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    Ok,
    /// (ADR-0022) 원격 mirror 응답 대기 중 — 로컬은 동기 IO 라 이 상태를 거치지 않는다.
    Loading,
    /// 권한 거부 (`PermissionDenied`).
    NoPermission,
    /// 그 외 IO 에러 (메시지).
    Error(String),
}

/// 원격 디렉터리 응답의 UI 대기 제한. 로컬 동기 읽기에는 적용하지 않는다.
const LIST_DIR_SOFT_TIMEOUT: Duration = Duration::from_secs(8);

/// 경로 하나의 원격 list_dir 요청 생애주기(ADR-0022 — `ExplorerView` 가
/// 자체 소유하는 경로별 pending 상태, host 범용 레지스트리 없음).
#[derive(Clone)]
enum RemoteLoadState {
    Loading {
        request_id: u64,
        sent_at: Instant,
    },
    /// 서버가 보낸 원본(Name/Asc) 엔트리 — main list/tree 소비처가 각자 필요한 형태로
    /// (재정렬/디렉토리만 필터) 파생한다.
    Loaded(Vec<DirEntryInfo>),
    Error(String),
}

/// 렌더 중 만든 원격 조회 요청. engine을 다시 빌릴 수 없어 루프 종료 뒤 outbox를 옮긴다.
pub(crate) struct ExplorerListRequest {
    pub(crate) local_ws_id: u32,
    pub(crate) request_id: u64,
    pub(crate) dir: PathBuf,
}

pub struct ExplorerView {
    /// 정렬된 현재 디렉토리 엔트리.
    pub entries: Vec<DirEntryInfo>,
    /// `entries` 가 어떤 디렉토리/정렬 기준으로 로드됐는지 (변화 감지용).
    loaded: Option<(PathBuf, SortColumn, SortDir)>,
    /// 로드 결과 상태.
    pub state: LoadState,
    /// 선택된 엔트리 경로 집합.
    pub selected: HashSet<PathBuf>,
    /// 마지막으로 클릭(앵커)된 엔트리 — shift 범위 선택 기준.
    pub anchor: Option<PathBuf>,
    /// 사이드바 디렉토리 트리에서 펼쳐진 디렉토리.
    pub expanded: HashSet<PathBuf>,
    /// 사이드바 트리 펼침으로 읽은 하위 디렉토리 캐시.
    pub tree_children: HashMap<PathBuf, Vec<DirEntryInfo>>,
    /// 강제 새로고침 요청 플래그 (F5 / refresh 버튼).
    reload_requested: bool,
    /// 주소창(PathField) 편집 버퍼. 비편집 시 `sync()` 가 활성 탭 cwd 로 재동기화한다.
    pub addr_buffer: String,
    /// 주소창 편집(=트리거 포커스) 여부. PathField 가 매 프레임 갱신.
    pub addr_editing: bool,
    /// 주소창 후보 드롭다운의 keyboard-active 행(필터된 가시 목록 기준).
    pub addr_active: Option<usize>,
    /// (ADR-0022) 이 surface 가 원격 mirror 인가 — `Some(local_ws_id)` 면 원격, `None`
    /// 이면 로컬. `sync()`가 매 호출마다 최신값으로 갱신한다.
    mirror_ws_id: Option<u32>,
    /// (ADR-0022) 경로별 원격 요청 상태 — 로컬 surface 는 항상 비어 있다.
    remote_state: HashMap<PathBuf, RemoteLoadState>,
    /// (ADR-0022) 이번 프레임 새로 만든 원격 요청 — 렌더 루프 종료 후 drain.
    outbox: Vec<ExplorerListRequest>,
    /// 타입어헤드 입력 버퍼(영숫자로 항목 선택). 규칙은 `type_ahead` 모듈에 있다.
    pub type_ahead: TypeAhead,
    /// 이번 프레임에 보이도록 스크롤할 항목 경로. **쓰고 나면 비운다** — 남겨두면
    /// 매 프레임 재스크롤이 되어 사용자가 휠로 다른 곳을 볼 때 끌려간다.
    pub scroll_to: Option<PathBuf>,
}

impl ExplorerView {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            loaded: None,
            state: LoadState::Ok,
            selected: HashSet::new(),
            anchor: None,
            expanded: HashSet::new(),
            tree_children: HashMap::new(),
            reload_requested: false,
            addr_buffer: String::new(),
            addr_editing: false,
            addr_active: None,
            mirror_ws_id: None,
            remote_state: HashMap::new(),
            outbox: Vec::new(),
            type_ahead: TypeAhead::default(),
            scroll_to: None,
        }
    }

    /// 타입어헤드 입력을 버린다. 내부 탭 전환/추가/닫기처럼 목록이 통째로 바뀌는
    /// 자리에서 호출한다 — 이전 탭에서 치던 접두사가 새 목록에 이어지면 안 된다.
    /// (디렉토리·정렬 변경은 `sync()` 가 스스로 처리한다.)
    pub fn reset_type_ahead(&mut self) {
        self.type_ahead.reset();
        self.scroll_to = None;
    }

    /// 주소창 편집을 취소한다. 내부 탭 전환/nav 로 cwd 가 바뀔 때 호출해 편집 버퍼가
    /// 다른 탭/경로로 새는 것을 막는다 — 다음 `sync()` 가 새 cwd 로 재동기화한다.
    pub fn cancel_addr_edit(&mut self) {
        self.addr_editing = false;
        self.addr_active = None;
    }

    /// 다음 렌더에서 현재 디렉토리를 다시 읽도록 표시.
    pub fn request_reload(&mut self) {
        self.reload_requested = true;
    }

    /// 현재 디렉토리의 모든 엔트리를 선택. 앵커는 마지막 엔트리로 둔다.
    pub fn select_all(&mut self) {
        self.selected = self.entries.iter().map(|e| e.path.clone()).collect();
        self.anchor = self.entries.last().map(|e| e.path.clone());
    }

    /// 선택된 경로를 (정렬·개행 결합) 텍스트로. 선택이 없으면 None.
    /// "경로 복사"(copy_path) 클립보드 페이로드.
    pub fn selected_paths_text(&self) -> Option<String> {
        if self.selected.is_empty() {
            return None;
        }
        let mut paths: Vec<String> = self
            .selected
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        paths.sort();
        Some(paths.join("\n"))
    }

    /// 활성 탭 기준으로 엔트리 캐시를 동기화. 로컬은 디렉토리/정렬이 바뀌었거나
    /// 새로고침이 요청됐으면 디스크에서 다시 읽는다. `mirror_ws_id` 가 `Some` 이면
    /// (ADR-0022) 원격 mirror surface — 동기 IO 대신 `list_dir_request` 를 큐잉하고
    /// 경로별 pending 상태로 진행 상황을 추적한다. 디렉토리가 바뀌면 선택을 초기화한다.
    pub fn sync(&mut self, panel: &ExplorerPanel, mirror_ws_id: Option<u32>) {
        let tab = panel.active_tab();
        // 편집 중에는 입력을 유지하고, 아니면 주소를 현재 cwd로 맞춘다. 목록 갱신과는 별개다.
        if !self.addr_editing {
            let cwd = tab.root.display().to_string();
            if self.addr_buffer != cwd {
                self.addr_buffer = cwd;
            }
            self.addr_active = None;
        }

        self.mirror_ws_id = mirror_ws_id;
        if let Some(local_ws_id) = mirror_ws_id {
            self.sync_remote(&tab.root, tab.sort_column, tab.sort_dir, local_ws_id);
            return;
        }

        let key = (tab.root.clone(), tab.sort_column, tab.sort_dir);
        let dir_changed = self
            .loaded
            .as_ref()
            .map(|(d, _, _)| d != &tab.root)
            .unwrap_or(true);
        let need = self.reload_requested || self.loaded.as_ref() != Some(&key);
        if !need {
            return;
        }
        self.reload_requested = false;
        // 목록이 바뀌는 것이 확정된 자리다. 정렬만 바뀌어도 인덱스 의미가 달라지므로
        // 디렉토리 변경(`dir_changed`)보다 넓은 이 조건에서 버퍼를 비운다.
        self.reset_type_ahead();
        if dir_changed {
            self.selected.clear();
            self.anchor = None;
        }
        match read_dir_entries(&tab.root) {
            Ok(mut entries) => {
                sort_entries(&mut entries, tab.sort_column, tab.sort_dir);
                self.entries = entries;
                self.state = LoadState::Ok;
            }
            Err(e) => {
                self.entries.clear();
                self.state = if e.kind() == std::io::ErrorKind::PermissionDenied {
                    LoadState::NoPermission
                } else {
                    LoadState::Error(e.to_string())
                };
            }
        }
        if dir_changed || self.tree_children.is_empty() {
            self.tree_children.clear();
        }
        self.loaded = Some(key);
    }

    /// 경로별 원격 요청을 만들거나 저장된 결과를 표시한다. 실제 응답 반영은 apply_remote_list_dir_result에서 한다.
    fn sync_remote(
        &mut self,
        dir: &Path,
        sort_column: SortColumn,
        sort_dir: SortDir,
        local_ws_id: u32,
    ) {
        let key = (dir.to_path_buf(), sort_column, sort_dir);
        let dir_changed = self
            .loaded
            .as_ref()
            .map(|(d, _, _)| d.as_path() != dir)
            .unwrap_or(true);
        let refresh = self.reload_requested;
        self.reload_requested = false;

        // 응답 대기가 제한 시간을 넘으면 조회 오류로 표시한다.
        if let Some(RemoteLoadState::Loading { sent_at, .. }) = self.remote_state.get(dir)
            && sent_at.elapsed() > LIST_DIR_SOFT_TIMEOUT
        {
            self.remote_state.insert(
                dir.to_path_buf(),
                RemoteLoadState::Error(t("explorer.state.error_conn_timeout").to_string()),
            );
        }

        let need_request = match self.remote_state.get(dir) {
            None => true,
            Some(RemoteLoadState::Loading { .. }) => false,
            Some(_) => refresh,
        };
        if need_request {
            let request_id = crate::core::next_list_dir_request_id();
            self.remote_state.insert(
                dir.to_path_buf(),
                RemoteLoadState::Loading {
                    request_id,
                    sent_at: Instant::now(),
                },
            );
            self.outbox.push(ExplorerListRequest {
                local_ws_id,
                request_id,
                dir: dir.to_path_buf(),
            });
        }

        if dir_changed {
            self.selected.clear();
            self.anchor = None;
            self.tree_children.clear();
        }

        self.state = match self.remote_state.get(dir) {
            Some(RemoteLoadState::Loading { .. }) | None => LoadState::Loading,
            Some(RemoteLoadState::Loaded(raw)) => {
                let mut entries = raw.clone();
                sort_entries(&mut entries, sort_column, sort_dir);
                self.entries = entries;
                LoadState::Ok
            }
            Some(RemoteLoadState::Error(msg)) => {
                self.entries.clear();
                if msg == "permission denied" {
                    LoadState::NoPermission
                } else {
                    LoadState::Error(msg.clone())
                }
            }
        };
        self.loaded = Some(key);
    }

    /// 현재 기다리는 요청 ID의 경로만 반환한다. 오래되거나 다른 요청이면 None이다.
    fn find_pending_dir(&self, request_id: u64) -> Option<PathBuf> {
        self.remote_state
            .iter()
            .find_map(|(dir, state)| match state {
                RemoteLoadState::Loading {
                    request_id: rid, ..
                } if *rid == request_id => Some(dir.clone()),
                _ => None,
            })
    }

    /// 기다리는 요청의 응답을 경로 캐시에 반영한다. 현재 디렉터리이면 본문도 갱신하며
    /// 트리와 본문이 같은 경로를 보고 있으면 같은 응답을 함께 사용한다.
    pub(crate) fn apply_remote_list_dir_result(
        &mut self,
        request_id: u64,
        panel: &ExplorerPanel,
        result: Result<Vec<DirEntryInfo>, String>,
    ) -> bool {
        let Some(dir) = self.find_pending_dir(request_id) else {
            return false;
        };
        let current_root = panel.current_root();
        let is_current = dir == current_root;
        match result {
            Ok(entries) => {
                let mut tree_children: Vec<DirEntryInfo> =
                    entries.iter().filter(|e| e.is_dir).cloned().collect();
                tree_children.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                self.tree_children.insert(dir.clone(), tree_children);
                if is_current {
                    let tab = panel.active_tab();
                    let mut sorted = entries.clone();
                    sort_entries(&mut sorted, tab.sort_column, tab.sort_dir);
                    self.entries = sorted;
                    self.state = LoadState::Ok;
                }
                self.remote_state
                    .insert(dir, RemoteLoadState::Loaded(entries));
            }
            Err(reason) => {
                if is_current {
                    self.entries.clear();
                    self.state = if reason == "permission denied" {
                        LoadState::NoPermission
                    } else {
                        LoadState::Error(reason.clone())
                    };
                }
                self.tree_children.insert(dir.clone(), Vec::new());
                self.remote_state
                    .insert(dir, RemoteLoadState::Error(reason));
            }
        }
        true
    }

    /// 이번 프레임 새로 만든 원격 list_dir 요청을 모두 꺼낸다(렌더 루프 종료 후
    /// 호출자가 `CoreState::pending_list_dir_forward` 로 옮긴다).
    pub(crate) fn drain_outbox(&mut self) -> Vec<ExplorerListRequest> {
        std::mem::take(&mut self.outbox)
    }

    /// 사이드바 트리에서 `dir` 의 하위 디렉토리를 (캐시에 없으면) 읽어 반환.
    /// `mirror_ws_id` 가 `Some` 이면(ADR-0022) 동기 IO 대신 `list_dir_request` 를
    /// 큐잉하고, 응답이 올 때까지 빈 슬라이스를 반환한다(다음 프레임들에서 자동 채움).
    pub fn tree_children_of(&mut self, dir: &Path, mirror_ws_id: Option<u32>) -> &[DirEntryInfo] {
        if let Some(local_ws_id) = mirror_ws_id {
            if !self.tree_children.contains_key(dir) {
                if self.remote_state.get(dir).is_none() {
                    let request_id = crate::core::next_list_dir_request_id();
                    self.remote_state.insert(
                        dir.to_path_buf(),
                        RemoteLoadState::Loading {
                            request_id,
                            sent_at: Instant::now(),
                        },
                    );
                    self.outbox.push(ExplorerListRequest {
                        local_ws_id,
                        request_id,
                        dir: dir.to_path_buf(),
                    });
                }
                // placeholder — 응답 도착 전엔 빈 슬라이스, 매 프레임 재요청 방지.
                self.tree_children.entry(dir.to_path_buf()).or_default();
            }
            return self
                .tree_children
                .get(dir)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
        }
        if !self.tree_children.contains_key(dir) {
            let children = read_dir_entries(dir)
                .map(|mut v| {
                    v.retain(|e| e.is_dir);
                    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                    v
                })
                .unwrap_or_default();
            self.tree_children.insert(dir.to_path_buf(), children);
        }
        &self.tree_children[dir]
    }

    /// 단일 선택으로 설정.
    pub fn select_only(&mut self, path: &Path) {
        self.selected.clear();
        self.selected.insert(path.to_path_buf());
        self.anchor = Some(path.to_path_buf());
    }

    /// 토글 선택 (ctrl-click).
    pub fn toggle_select(&mut self, path: &Path) {
        if !self.selected.remove(path) {
            self.selected.insert(path.to_path_buf());
        }
        self.anchor = Some(path.to_path_buf());
    }
}

impl Default for ExplorerView {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
pub struct ExplorerViewStore {
    views: HashMap<SurfaceId, ExplorerView>,
}

impl ExplorerViewStore {
    /// surface 의 뷰를 가져오고 (없으면 생성) 활성 탭 기준으로 동기화. `mirror_ws_id`
    /// 가 `Some` 이면(ADR-0022) 이 surface 가 속한 mirror workspace id — view 는 동기
    /// 로컬 IO 대신 원격 `list_dir_request` 를 큐잉한다.
    pub fn get_or_init(
        &mut self,
        panel: &ExplorerPanel,
        mirror_ws_id: Option<u32>,
    ) -> &mut ExplorerView {
        let view = self.views.entry(panel.id).or_default();
        view.sync(panel, mirror_ws_id);
        view
    }

    pub fn get(&self, sid: SurfaceId) -> Option<&ExplorerView> {
        self.views.get(&sid)
    }

    pub fn get_mut(&mut self, sid: SurfaceId) -> Option<&mut ExplorerView> {
        self.views.get_mut(&sid)
    }

    /// (ADR-0022) `request_id` 로 대기 중인 view 를 찾아 응답을 반영한다. 어느 view도
    /// 이 `request_id` 를 기다리지 않았으면(stale) `false`.
    pub(crate) fn apply_remote_list_dir_result(
        &mut self,
        surface_id: SurfaceId,
        request_id: u64,
        panel: &ExplorerPanel,
        result: Result<Vec<DirEntryInfo>, String>,
    ) -> bool {
        self.views
            .get_mut(&surface_id)
            .map(|v| v.apply_remote_list_dir_result(request_id, panel, result))
            .unwrap_or(false)
    }

    /// 모든 view 에서 이번 프레임 새로 만든 원격 list_dir 요청을 `(surface_id, request)`
    /// 쌍으로 drain 한다 — 렌더 루프(engine 재차입 불가) 종료 후 호출자가
    /// `CoreState::pending_list_dir_forward` 로 옮긴다.
    pub(crate) fn drain_outbox(&mut self) -> Vec<(SurfaceId, ExplorerListRequest)> {
        let mut out = Vec::new();
        for (&sid, view) in self.views.iter_mut() {
            for req in view.drain_outbox() {
                out.push((sid, req));
            }
        }
        out
    }

    pub fn drop_view(&mut self, sid: SurfaceId) {
        self.views.remove(&sid);
    }

    /// 현재 뷰 개수. system.gpu_stats에서 닫힌 surface의 상태 정리를 확인할 때 쓴다.
    pub(crate) fn view_count(&self) -> usize {
        self.views.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_units() {
        assert_eq!(human_size(true, 999), "—");
        assert_eq!(human_size(false, 512), "512 B");
        assert_eq!(human_size(false, 4096), "4.0 KB");
    }

    #[test]
    fn sort_dirs_first() {
        let mut v = vec![
            DirEntryInfo {
                path: "/z".into(),
                name: "z".into(),
                is_dir: false,
                size: 1,
                modified: None,
                ext: String::new(),
            },
            DirEntryInfo {
                path: "/a".into(),
                name: "a".into(),
                is_dir: true,
                size: 0,
                modified: None,
                ext: String::new(),
            },
        ];
        sort_entries(&mut v, SortColumn::Name, SortDir::Asc);
        assert!(v[0].is_dir);
    }

    /// 비편집 시 sync 가 주소창 버퍼를 활성 탭 cwd 로 맞춘다.
    #[test]
    fn sync_addr_buffer_tracks_cwd_when_not_editing() {
        let panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
        let mut view = ExplorerView::new();
        view.sync(&panel, None);
        assert_eq!(view.addr_buffer, "/tmp/alpha");
        assert!(!view.addr_editing);
    }

    /// 편집 중이면 sync 가 cwd 로 덮어쓰지 않고 사용자 입력을 보존한다.
    #[test]
    fn sync_preserves_addr_buffer_while_editing() {
        let mut panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
        let mut view = ExplorerView::new();
        view.sync(&panel, None);
        view.addr_editing = true;
        view.addr_buffer = "/tmp/typed".to_string();
        panel
            .active_tab_mut()
            .navigate_to(PathBuf::from("/tmp/beta"));
        view.sync(&panel, None);
        assert_eq!(view.addr_buffer, "/tmp/typed");
    }

    /// 목록이 다시 적재되면 타입어헤드 버퍼가 비워진다 — 새 디렉토리에서 옛 접두사가
    /// 이어지면 사용자가 치지 않은 글자로 검색하는 것이 된다.
    ///
    /// 버퍼는 사적이라 값으로 못 보고, 다음 입력이 무엇으로 검색되는지로 잰다:
    /// 비워지지 않았으면 "ab" 로 찾아 `None`, 비워졌으면 "b" 로 찾아 `bravo`.
    #[test]
    fn reloading_the_listing_clears_the_type_ahead_buffer() {
        let names = vec!["alpha".to_string(), "bravo".to_string()];
        let panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
        let mut view = ExplorerView::new();
        view.sync(&panel, None);

        let now = std::time::Instant::now();
        assert_eq!(view.type_ahead.feed('a', now, &names, None), Some(0));

        // 정렬만 바꿔도 인덱스 의미가 달라지므로 같은 자리에서 비워져야 한다.
        let mut panel = panel;
        panel.active_tab_mut().sort_dir = SortDir::Desc;
        view.sync(&panel, None);

        assert_eq!(view.type_ahead.feed('b', now, &names, None), Some(1));
    }

    /// cancel_addr_edit 후 sync 가 새 cwd 로 재동기화(내부 탭 전환/nav 누수 방지).
    #[test]
    fn cancel_addr_edit_resyncs_to_new_cwd() {
        let mut panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
        let mut view = ExplorerView::new();
        view.sync(&panel, None);
        view.addr_editing = true;
        view.addr_buffer = "/tmp/typed".to_string();
        panel
            .active_tab_mut()
            .navigate_to(PathBuf::from("/tmp/beta"));
        view.cancel_addr_edit();
        assert!(!view.addr_editing);
        view.sync(&panel, None);
        assert_eq!(view.addr_buffer, "/tmp/beta");
    }

    /// (ADR-0022) `mirror_ws_id` 가 `Some` 이면 동기 IO 대신 `list_dir_request` 를
    /// outbox 에 큐잉하고 `LoadState::Loading` 으로 전이한다.
    #[test]
    fn sync_remote_queues_request_and_sets_loading() {
        let panel = ExplorerPanel::new(1, PathBuf::from("/remote/project"));
        let mut view = ExplorerView::new();
        view.sync(&panel, Some(7));
        assert_eq!(view.state, LoadState::Loading);
        let drained = view.drain_outbox();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].local_ws_id, 7);
        assert_eq!(drained[0].dir, PathBuf::from("/remote/project"));
        assert!(view.drain_outbox().is_empty());
    }

    /// (ADR-0022) 이 view 가 실제로 기다리던 request_id 가 아니면(stale)
    /// 조용히 무시 — `entries`/`state` 를 바꾸지 않고 `false` 를 반환한다.
    #[test]
    fn apply_remote_list_dir_result_ignores_stale_request_id() {
        let panel = ExplorerPanel::new(1, PathBuf::from("/remote/project"));
        let mut view = ExplorerView::new();
        view.sync(&panel, Some(7));
        let real_request_id = match view.remote_state.get(panel.current_root()) {
            Some(RemoteLoadState::Loading { request_id, .. }) => *request_id,
            _ => panic!("expected pending Loading state"),
        };
        let stale_request_id = real_request_id.wrapping_add(1);
        let applied = view.apply_remote_list_dir_result(stale_request_id, &panel, Ok(Vec::new()));
        assert!(!applied);
        assert_eq!(view.state, LoadState::Loading);
        assert!(view.entries.is_empty());
    }

    /// 실제로 기다리던 request_id 로 응답이 오면 현재 root 의 `entries`/`state` 를
    /// 갱신하고 `tree_children` 캐시도 함께 채운다(메인 목록·트리가 같은 응답을 공유).
    #[test]
    fn apply_remote_list_dir_result_updates_entries_on_matching_request_id() {
        let panel = ExplorerPanel::new(1, PathBuf::from("/remote/project"));
        let mut view = ExplorerView::new();
        view.sync(&panel, Some(7));
        let request_id = match view.remote_state.get(panel.current_root()) {
            Some(RemoteLoadState::Loading { request_id, .. }) => *request_id,
            _ => panic!("expected pending Loading state"),
        };
        let entries = vec![DirEntryInfo {
            path: "/remote/project/a".into(),
            name: "a".into(),
            is_dir: true,
            size: 0,
            modified: None,
            ext: String::new(),
        }];
        let applied = view.apply_remote_list_dir_result(request_id, &panel, Ok(entries.clone()));
        assert!(applied);
        assert_eq!(view.state, LoadState::Ok);
        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.entries[0].name, "a");
        assert_eq!(
            view.tree_children
                .get(panel.current_root())
                .map(|v| v.len()),
            Some(1)
        );
    }

    /// 실패 응답(`"permission denied"`)은 `LoadState::NoPermission` 으로, 그 외 사유는
    /// `LoadState::Error` 로 반영한다(File Picker 와 동일한 문자열 비교 판정).
    #[test]
    fn apply_remote_list_dir_result_maps_permission_denied_reason() {
        let panel = ExplorerPanel::new(1, PathBuf::from("/remote/project"));
        let mut view = ExplorerView::new();
        view.sync(&panel, Some(7));
        let request_id = match view.remote_state.get(panel.current_root()) {
            Some(RemoteLoadState::Loading { request_id, .. }) => *request_id,
            _ => panic!("expected pending Loading state"),
        };
        let applied = view.apply_remote_list_dir_result(
            request_id,
            &panel,
            Err("permission denied".to_string()),
        );
        assert!(applied);
        assert_eq!(view.state, LoadState::NoPermission);
    }
}
