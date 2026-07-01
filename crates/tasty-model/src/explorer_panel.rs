//! `ExplorerPanel` — 본체 내장 파일 관리자 surface (T11).
//!
//! image/markdown panel 과 같은 패턴: panel 은 **식별 + 내비게이션 상태만** 보유하고,
//! 무거운 view state (디렉토리 엔트리 캐시, 선택 집합, 스크롤, 트리 펼침)는 host 의
//! `ExplorerView` (`src/adapters/ui/surface/explorer/view.rs`) 에 둔다.
//!
//! surface 복구(restore) 시 내부 탭 목록이 함께 복구돼야 하므로(결정 3), 직렬화
//! 대상인 **내부 탭(root 경로 + view_mode + 정렬)과 활성 탭 인덱스**는 panel 이 들고
//! 있는다. snapshot/restore 의 JSON 변환은 host 의 `register_explorer`
//! (`src/engine/surface_registry/builtins.rs`) 가 담당해 본 crate 는 GUI/serde 무관을
//! 유지한다.

use std::path::{Path, PathBuf};

use super::SurfaceId;
use super::surface_trait::Surface;

/// content view 표현 방식 (design §3.2 — grid / list / detail).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerViewMode {
    /// 균등 아이콘 그리드.
    Grid,
    /// 단일 컬럼 목록.
    List,
    /// 정렬 가능한 다중 컬럼 (Name/Size/Modified/Type).
    Detail,
}

impl ExplorerViewMode {
    /// snapshot 직렬화용 안정 식별자.
    pub fn as_str(self) -> &'static str {
        match self {
            ExplorerViewMode::Grid => "grid",
            ExplorerViewMode::List => "list",
            ExplorerViewMode::Detail => "detail",
        }
    }

    /// 식별자 → 모드. 알 수 없으면 `Detail` (와이어프레임 기본).
    // 무한 실패(default fallback) 파서라 `FromStr`(fallible)과 시그니처가 맞지 않고
    // `as_str` 과 대칭을 이루는 의도된 API 이므로 trait 구현 권고를 끈다.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "grid" => ExplorerViewMode::Grid,
            "list" => ExplorerViewMode::List,
            _ => ExplorerViewMode::Detail,
        }
    }
}

/// detail 뷰 정렬 기준 컬럼.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
    Type,
}

impl SortColumn {
    pub fn as_str(self) -> &'static str {
        match self {
            SortColumn::Name => "name",
            SortColumn::Size => "size",
            SortColumn::Modified => "modified",
            SortColumn::Type => "type",
        }
    }

    // 무한 실패(default fallback) 파서라 `FromStr`(fallible)과 시그니처가 맞지 않고
    // `as_str` 과 대칭을 이루는 의도된 API 이므로 trait 구현 권고를 끈다.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "size" => SortColumn::Size,
            "modified" => SortColumn::Modified,
            "type" => SortColumn::Type,
            _ => SortColumn::Name,
        }
    }
}

/// 정렬 방향.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    pub fn as_str(self) -> &'static str {
        match self {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        }
    }

    // 무한 실패(default fallback) 파서라 `FromStr`(fallible)과 시그니처가 맞지 않고
    // `as_str` 과 대칭을 이루는 의도된 API 이므로 trait 구현 권고를 끈다.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "desc" => SortDir::Desc,
            _ => SortDir::Asc,
        }
    }

    /// 토글 (asc ↔ desc).
    pub fn toggled(self) -> Self {
        match self {
            SortDir::Asc => SortDir::Desc,
            SortDir::Desc => SortDir::Asc,
        }
    }
}

/// explorer 내부 탭 하나 — surface-local (결정 3).
///
/// **cwd(고정 루트) ↔ current(현재 폴더) 분리**: `cwd` 는 explorer 를 연 프로젝트 루트로
/// 좌측 트리·스폰 cwd 의 기준이며 내비게이션에 불변. `root`(current) 는 우측 목록·상단
/// breadcrumb 이 따라가는 탐색 폴더로, back/forward/go_up 이 이것만 움직인다. current 는
/// cwd 하위로 제한되지 않고 파일시스템 어디로든 자유 이동할 수 있다(결정1).
/// 선택·스크롤 같은 무거운 상태는 `ExplorerView` 에 둔다.
#[derive(Clone, Debug)]
pub struct ExplorerTab {
    /// 고정 루트(cwd) — explorer 를 연 프로젝트 폴더. 좌측 트리 루트 + 스폰 cwd 의 기준.
    /// `set_cwd` 이외에는 불변.
    pub cwd: PathBuf,
    /// 현재 표시 중인 디렉토리(current). 우측 목록·breadcrumb 이 따라간다.
    pub root: PathBuf,
    /// 뒤로 가기 히스토리 (가장 최근이 끝).
    back: Vec<PathBuf>,
    /// 앞으로 가기 히스토리.
    forward: Vec<PathBuf>,
    /// 표시 모드.
    pub view_mode: ExplorerViewMode,
    /// detail 정렬 컬럼.
    pub sort_column: SortColumn,
    /// detail 정렬 방향.
    pub sort_dir: SortDir,
}

impl ExplorerTab {
    /// cwd = current = `root` 로 초기화. (프로젝트를 연 직후 두 위치가 같다.)
    pub fn new(root: PathBuf) -> Self {
        Self {
            cwd: root.clone(),
            root,
            back: Vec::new(),
            forward: Vec::new(),
            view_mode: ExplorerViewMode::Detail,
            sort_column: SortColumn::Name,
            sort_dir: SortDir::Asc,
        }
    }

    /// 명시적으로 cwd 와 current 를 따로 지정해 복원한다(snapshot restore 용).
    pub fn with_cwd(cwd: PathBuf, current: PathBuf) -> Self {
        Self {
            cwd,
            root: current,
            back: Vec::new(),
            forward: Vec::new(),
            view_mode: ExplorerViewMode::Detail,
            sort_column: SortColumn::Name,
            sort_dir: SortDir::Asc,
        }
    }

    /// 고정 루트(cwd).
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// 현재 폴더(current).
    pub fn current(&self) -> &Path {
        &self.root
    }

    /// cwd 를 `folder` 로 재설정. current 도 folder 로 맞추고 히스토리를 비운다.
    /// (explorer-03 "이 폴더로 루트 설정" 이 사용.)
    pub fn set_cwd(&mut self, folder: PathBuf) {
        self.cwd = folder.clone();
        self.root = folder;
        self.back.clear();
        self.forward.clear();
    }

    /// 새 디렉토리로 이동. 현재 root 를 back 에 push 하고 forward 를 비운다.
    pub fn navigate_to(&mut self, dir: PathBuf) {
        if dir == self.root {
            return;
        }
        self.back.push(std::mem::replace(&mut self.root, dir));
        self.forward.clear();
    }

    /// 부모 디렉토리로 이동. 루트면 no-op, 이동했으면 true.
    pub fn go_up(&mut self) -> bool {
        if let Some(parent) = self.root.parent().map(|p| p.to_path_buf()) {
            self.navigate_to(parent);
            true
        } else {
            false
        }
    }

    /// 뒤로 가기. 이동했으면 true.
    pub fn go_back(&mut self) -> bool {
        if let Some(prev) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.root, prev));
            true
        } else {
            false
        }
    }

    /// 앞으로 가기. 이동했으면 true.
    pub fn go_forward(&mut self) -> bool {
        if let Some(next) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.root, next));
            true
        } else {
            false
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn can_go_up(&self) -> bool {
        self.root.parent().is_some()
    }
}

/// 본체 내장 파일 관리자 surface. 내부 탭 목록 + 활성 탭 인덱스만 보유.
pub struct ExplorerPanel {
    pub id: u32,
    /// surface-local 내부 탭 (최소 1개 보장).
    pub tabs: Vec<ExplorerTab>,
    /// 활성 내부 탭 인덱스.
    pub active: usize,
}

impl ExplorerPanel {
    /// 단일 탭으로 시작하는 explorer surface.
    pub fn new(id: u32, root: PathBuf) -> Self {
        Self {
            id,
            tabs: vec![ExplorerTab::new(root)],
            active: 0,
        }
    }

    /// 여러 탭으로 복원. `tabs` 가 비어 있으면 home 단일 탭으로 보정.
    pub fn from_tabs(id: u32, tabs: Vec<ExplorerTab>, active: usize) -> Self {
        let tabs = if tabs.is_empty() {
            vec![ExplorerTab::new(default_root())]
        } else {
            tabs
        };
        let active = active.min(tabs.len() - 1);
        Self { id, tabs, active }
    }

    pub fn active_tab(&self) -> &ExplorerTab {
        // `active` 는 항상 유효 범위 (tabs 비지 않음 보장).
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }

    pub fn active_tab_mut(&mut self) -> &mut ExplorerTab {
        let idx = self.active.min(self.tabs.len() - 1);
        self.active = idx;
        &mut self.tabs[idx]
    }

    /// 현재 활성 탭의 cwd 를 복제해 새 내부 탭을 추가하고 활성화한다(current = cwd).
    pub fn add_tab(&mut self) {
        let cwd = self.active_tab().cwd.clone();
        self.tabs.push(ExplorerTab::new(cwd));
        self.active = self.tabs.len() - 1;
    }

    /// 내부 탭 닫기. 마지막 1개는 닫지 않는다(최소 1개 보장).
    pub fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if idx < self.active {
            self.active -= 1;
        }
    }

    /// 현재 활성 탭의 current(현재 폴더) 경로. content/breadcrumb 용.
    pub fn current_root(&self) -> &Path {
        &self.active_tab().root
    }

    /// 현재 활성 탭의 cwd(고정 루트). 좌측 트리 루트 + 스폰 cwd 용.
    pub fn cwd(&self) -> &Path {
        &self.active_tab().cwd
    }
}

impl Surface for ExplorerPanel {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "explorer"
    }
    fn type_name(&self) -> &'static str {
        "Explorer"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
    fn display_name(&self) -> String {
        // surface 표시명은 안정적인 프로젝트 정체성(cwd) 기준. 현재 폴더는 content/
        // breadcrumb 이 보여주므로 표시명은 고정 cwd 이름으로 둔다.
        let cwd = self.cwd();
        cwd.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cwd.to_string_lossy().to_string())
    }
    fn source_cwd(&self) -> Option<PathBuf> {
        // 스폰 cwd 는 고정 프로젝트 루트(current 서브폴더가 아님) — 결정2.
        Some(self.cwd().to_path_buf())
    }
}

/// 경로 미지정 복원 시의 안전 fallback root. 홈 디렉토리 해석은 호스트
/// (`register_explorer` / IPC `tab.rs`) 책임이며, 본 fallback 은 빈 탭 목록으로
/// 복원되는 엣지 케이스에서만 쓰인다.
pub fn default_root() -> PathBuf {
    PathBuf::from(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_pushes_history() {
        let mut t = ExplorerTab::new(PathBuf::from("/a"));
        t.navigate_to(PathBuf::from("/a/b"));
        assert_eq!(t.root, PathBuf::from("/a/b"));
        assert!(t.can_go_back());
        assert!(!t.can_go_forward());
        assert!(t.go_back());
        assert_eq!(t.root, PathBuf::from("/a"));
        assert!(t.can_go_forward());
        assert!(t.go_forward());
        assert_eq!(t.root, PathBuf::from("/a/b"));
    }

    #[test]
    fn navigate_clears_forward() {
        let mut t = ExplorerTab::new(PathBuf::from("/a"));
        t.navigate_to(PathBuf::from("/a/b"));
        t.go_back();
        t.navigate_to(PathBuf::from("/a/c"));
        assert!(!t.can_go_forward());
    }

    #[test]
    fn close_tab_keeps_minimum_one() {
        let mut p = ExplorerPanel::new(1, PathBuf::from("/a"));
        p.close_tab(0);
        assert_eq!(p.tabs.len(), 1);
    }

    #[test]
    fn add_and_close_tab_adjusts_active() {
        let mut p = ExplorerPanel::new(1, PathBuf::from("/a"));
        p.add_tab(); // active = 1
        assert_eq!(p.active, 1);
        p.close_tab(1);
        assert_eq!(p.active, 0);
        assert_eq!(p.tabs.len(), 1);
    }

    #[test]
    fn navigation_moves_current_but_cwd_is_fixed() {
        let mut t = ExplorerTab::new(PathBuf::from("/proj"));
        assert_eq!(t.cwd(), Path::new("/proj"));
        assert_eq!(t.current(), Path::new("/proj"));
        t.navigate_to(PathBuf::from("/proj/sub"));
        assert_eq!(t.cwd(), Path::new("/proj")); // cwd 불변
        assert_eq!(t.current(), Path::new("/proj/sub")); // current 이동
        // 자유 이동: cwd 바깥(상위)도 허용
        t.navigate_to(PathBuf::from("/"));
        assert_eq!(t.cwd(), Path::new("/proj")); // 여전히 고정
        assert_eq!(t.current(), Path::new("/"));
    }

    #[test]
    fn set_cwd_resets_both_and_clears_history() {
        let mut t = ExplorerTab::new(PathBuf::from("/proj"));
        t.navigate_to(PathBuf::from("/proj/sub"));
        assert!(t.can_go_back());
        t.set_cwd(PathBuf::from("/other"));
        assert_eq!(t.cwd(), Path::new("/other"));
        assert_eq!(t.current(), Path::new("/other"));
        assert!(!t.can_go_back());
        assert!(!t.can_go_forward());
    }

    #[test]
    fn source_cwd_returns_fixed_cwd() {
        let mut p = ExplorerPanel::new(1, PathBuf::from("/proj"));
        p.active_tab_mut().navigate_to(PathBuf::from("/proj/sub"));
        assert_eq!(p.source_cwd(), Some(PathBuf::from("/proj")));
        assert_eq!(p.current_root(), Path::new("/proj/sub"));
    }

    #[test]
    fn add_tab_clones_cwd_and_resets_current() {
        let mut p = ExplorerPanel::new(1, PathBuf::from("/proj"));
        p.active_tab_mut().navigate_to(PathBuf::from("/proj/sub"));
        p.add_tab();
        assert_eq!(p.cwd(), Path::new("/proj")); // cwd 복제
        assert_eq!(p.current_root(), Path::new("/proj")); // current = cwd
    }

    #[test]
    fn view_mode_round_trips() {
        for m in [
            ExplorerViewMode::Grid,
            ExplorerViewMode::List,
            ExplorerViewMode::Detail,
        ] {
            assert_eq!(ExplorerViewMode::from_str(m.as_str()), m);
        }
    }
}
