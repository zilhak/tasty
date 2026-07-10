//! Host-side per-surface state for `ExplorerPanel` rendering (T11).
//!
//! `ExplorerPanel` (model) 은 식별 + 내비게이션(내부 탭/히스토리/뷰모드)만 보유하고,
//! 디렉토리 엔트리 캐시·선택 집합·사이드바 트리 펼침 같은 무거운 GUI 상태는 본 뷰
//! 스토어에 둔다 (markdown/image 뷰 스토어와 동형). surface id 로 keying.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tasty_model::{ExplorerPanel, SortColumn, SortDir, SurfaceId};

/// 디렉토리 엔트리 한 줄의 메타데이터 (디스크에서 1회 읽어 캐시).
#[derive(Clone)]
pub struct DirEntryInfo {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// 파일 바이트 크기 (디렉토리는 0).
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// 소문자 확장자 (없으면 빈 문자열).
    pub ext: String,
}

/// 디렉토리 로드 결과 상태 (content 중앙 상태 텍스트로 표현 — design §3.8).
#[derive(Clone, PartialEq, Eq)]
pub enum LoadState {
    Ok,
    /// 권한 거부 (`PermissionDenied`).
    NoPermission,
    /// 그 외 IO 에러 (메시지).
    Error(String),
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
        }
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

    /// 활성 탭 기준으로 엔트리 캐시를 동기화. 디렉토리/정렬이 바뀌었거나 새로고침이
    /// 요청됐으면 디스크에서 다시 읽는다. 디렉토리가 바뀌면 선택을 초기화한다.
    pub fn sync(&mut self, panel: &ExplorerPanel) {
        let tab = panel.active_tab();
        // 주소창: 비편집 시 항상 활성 탭 cwd 로 재동기화(탭 전환/nav 로 cwd 가 바뀌면 버퍼도
        // 따라간다). 편집 중이면 사용자 입력을 보존한다. 엔트리 reload 여부와 독립이므로
        // 아래 early-return 보다 앞에 둔다.
        if !self.addr_editing {
            let cwd = tab.root.display().to_string();
            if self.addr_buffer != cwd {
                self.addr_buffer = cwd;
            }
            self.addr_active = None;
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
        // 트리 펼침 캐시도 새로고침 시 무효화.
        if dir_changed || self.tree_children.is_empty() {
            // (펼침 자체는 유지 — 다음 트리 렌더에서 lazy 재로드)
            self.tree_children.clear();
        }
        self.loaded = Some(key);
    }

    /// 사이드바 트리에서 `dir` 의 하위 디렉토리를 (캐시에 없으면) 읽어 반환.
    pub fn tree_children_of(&mut self, dir: &Path) -> &[DirEntryInfo] {
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
    /// surface 의 뷰를 가져오고 (없으면 생성) 활성 탭 기준으로 동기화.
    pub fn get_or_init(&mut self, panel: &ExplorerPanel) -> &mut ExplorerView {
        let view = self.views.entry(panel.id).or_default();
        view.sync(panel);
        view
    }

    pub fn get(&self, sid: SurfaceId) -> Option<&ExplorerView> {
        self.views.get(&sid)
    }

    pub fn get_mut(&mut self, sid: SurfaceId) -> Option<&mut ExplorerView> {
        self.views.get_mut(&sid)
    }

    pub fn drop_view(&mut self, sid: SurfaceId) {
        self.views.remove(&sid);
    }
}

/// 디렉토리 엔트리를 읽어 메타데이터로 변환. 숨김 파일은 포함(필터는 뷰 레벨).
fn read_dir_entries(dir: &Path) -> std::io::Result<Vec<DirEntryInfo>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = meta.as_ref().and_then(|m| m.modified().ok());
        let ext = if is_dir {
            String::new()
        } else {
            path.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default()
        };
        out.push(DirEntryInfo {
            path,
            name,
            is_dir,
            size,
            modified,
            ext,
        });
    }
    Ok(out)
}

/// 엔트리 정렬: 디렉토리 우선, 그 다음 선택된 컬럼/방향.
fn sort_entries(entries: &mut [DirEntryInfo], col: SortColumn, dir: SortDir) {
    entries.sort_by(|a, b| {
        // 디렉토리는 항상 위 (방향 무관).
        if a.is_dir != b.is_dir {
            return b.is_dir.cmp(&a.is_dir);
        }
        let ord = match col {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.size.cmp(&b.size),
            SortColumn::Modified => a.modified.cmp(&b.modified),
            SortColumn::Type => a
                .ext
                .cmp(&b.ext)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        };
        match dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    });
}

/// 사람이 읽는 파일 크기 (e.g. "4 KB"). 디렉토리는 "—".
pub fn human_size(is_dir: bool, size: u64) -> String {
    if is_dir {
        return "—".to_string();
    }
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut s = size as f64;
    let mut u = 0;
    while s >= 1024.0 && u < UNITS.len() - 1 {
        s /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} {}", size, UNITS[0])
    } else {
        format!("{:.1} {}", s, UNITS[u])
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
        view.sync(&panel);
        assert_eq!(view.addr_buffer, "/tmp/alpha");
        assert!(!view.addr_editing);
    }

    /// 편집 중이면 sync 가 cwd 로 덮어쓰지 않고 사용자 입력을 보존한다.
    #[test]
    fn sync_preserves_addr_buffer_while_editing() {
        let mut panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
        let mut view = ExplorerView::new();
        view.sync(&panel);
        // 편집 진입 후 타이핑.
        view.addr_editing = true;
        view.addr_buffer = "/tmp/typed".to_string();
        // cwd 가 바뀌어도(다른 탭/nav) 편집 중이면 버퍼 유지.
        panel
            .active_tab_mut()
            .navigate_to(PathBuf::from("/tmp/beta"));
        view.sync(&panel);
        assert_eq!(view.addr_buffer, "/tmp/typed");
    }

    /// cancel_addr_edit 후 sync 가 새 cwd 로 재동기화(내부 탭 전환/nav 누수 방지).
    #[test]
    fn cancel_addr_edit_resyncs_to_new_cwd() {
        let mut panel = ExplorerPanel::new(1, PathBuf::from("/tmp/alpha"));
        let mut view = ExplorerView::new();
        view.sync(&panel);
        view.addr_editing = true;
        view.addr_buffer = "/tmp/typed".to_string();
        panel
            .active_tab_mut()
            .navigate_to(PathBuf::from("/tmp/beta"));
        // 탭 전환/nav 적용 시 편집 취소 → 다음 sync 가 새 cwd 로 맞춘다.
        view.cancel_addr_edit();
        assert!(!view.addr_editing);
        view.sync(&panel);
        assert_eq!(view.addr_buffer, "/tmp/beta");
    }
}
