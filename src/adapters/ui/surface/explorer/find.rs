//! Find 바 — 지금 폴더를 이름으로 거르고, Subfolders 를 켜면 하위 폴더까지 찾는다.
//! 바는 툴바 아래에 붙고, 열어 둔 폴더를 떠나면(뒤로·위로·주소창·폴더 열기) 닫힌다.
//! 하위 폴더 검색은 로컬 읽기 worker 가 하고 결과는 들어오는 대로 목록을 채운다. 원격 explorer 는 거르기만 한다.

use std::path::{Path, PathBuf};

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    ExplorerFindBar, ExplorerFindEvents, ExplorerFindLabels, ExplorerFindStatus, explorer_find_bar,
    explorer_match_job, match_range,
};

use super::DirEntryInfo;
use super::view::ExplorerView;
use crate::app::local_reads::{SearchEvent, SearchQuery};
use crate::core::fs_list::sort_entries;
use crate::i18n::{t, t_fmt, t_fmt2};

/// 열려 있는 Find 바. 연 폴더를 함께 둔다.
pub(crate) struct FindState {
    root: PathBuf,
    query: String,
    deep: bool,
    /// 다음 프레임에 입력에 포커스를 준다.
    focus: bool,
    /// 입력 칸이 키를 받고 있다. 그동안 타입어헤드와 목록 단축키를 멈춘다.
    pub(crate) field_focused: bool,
    search: Option<Search>,
}

struct Search {
    receipt: Option<SearchQuery>,
    hits: Vec<DirEntryInfo>,
    skipped: Vec<PathBuf>,
    outcome: Outcome,
}

#[derive(Clone, PartialEq)]
enum Outcome {
    Running,
    Done,
    Stopped,
    Failed(String),
}

/// 목록 대신 보일 Find 상태 화면.
pub(super) enum FindScreen {
    /// 맞는 항목이 없다. 하위 폴더까지 찾았으면 true.
    NoMatches {
        folder: String,
        deep: bool,
    },
    Failed(String),
}

impl ExplorerView {
    /// Find 바를 열고 입력에 포커스를 준다. 이미 열려 있으면 포커스만 준다.
    pub(crate) fn open_find(&mut self) {
        let Some(root) = self.shown_dir().map(Path::to_path_buf) else {
            return;
        };
        match &mut self.find {
            Some(find) => find.focus = true,
            None => {
                self.find = Some(FindState {
                    root,
                    query: String::new(),
                    deep: false,
                    focus: true,
                    field_focused: false,
                    search: None,
                })
            }
        }
    }

    /// 툴바 Find 토글. 열려 있으면 닫는다.
    pub(crate) fn toggle_find(&mut self) {
        if self.find.is_some() {
            self.find = None;
        } else {
            self.open_find();
        }
    }

    /// 지금 폴더를 떠났으면 Find 바를 닫는다. 새로고침이면 하위 폴더 검색을 다시 한다.
    pub(super) fn sync_find(&mut self, root: &Path, reload: bool) {
        if self.find.as_ref().is_some_and(|f| f.root != root) {
            self.find = None;
        }
        if reload && let Some(find) = &mut self.find {
            find.restart();
        }
    }

    /// 목록에 보일 항목. 거르는 중이면 맞는 항목만, 하위 폴더 검색이면 그 결과다.
    pub(crate) fn shown_entries(&self) -> Vec<DirEntryInfo> {
        match &self.find {
            Some(find) if find.query.is_empty() => self.entries.clone(),
            Some(FindState {
                search: Some(search),
                ..
            }) => search.hits.clone(),
            Some(find) if !find.deep => self
                .entries
                .iter()
                .filter(|e| match_range(&e.name, &find.query).is_some())
                .cloned()
                .collect(),
            _ => self.entries.clone(),
        }
    }

    /// 하위 폴더 검색 결과. 검색 중이 아니면 비어 있다.
    pub(super) fn search_hits(&self) -> &[DirEntryInfo] {
        self.find
            .as_ref()
            .and_then(|f| f.search.as_ref())
            .map_or(&[], |s| s.hits.as_slice())
    }

    /// 이름에서 칠할 검색어. 거르거나 찾는 중이 아니면 빈 문자열이다.
    pub(super) fn find_query(&self) -> &str {
        self.find.as_ref().map_or("", |f| f.query.as_str())
    }

    /// 하위 폴더 검색 결과를 보이는 중이면 시작 폴더.
    pub(super) fn search_root(&self) -> Option<&Path> {
        self.find
            .as_ref()
            .filter(|f| f.search.is_some())
            .map(|f| f.root.as_path())
    }

    /// 상태줄 글자.
    pub(super) fn status_text(&self) -> String {
        let shown = self.shown_entries();
        let sel = self.selected.len();
        if let Some(root) = self.search_root()
            && sel == 1
            && let Some(path) = self.selected.iter().next()
        {
            return relative_path(root, path);
        }
        if sel > 0 {
            return t_fmt("explorer.status.selected", &sel.to_string());
        }
        match &self.find {
            Some(find) if find.search.is_some() => {
                t_fmt("explorer.find.found", &shown.len().to_string())
            }
            Some(find) if !find.query.is_empty() => t_fmt2(
                "explorer.find.status",
                &shown.len().to_string(),
                &self.entries.len().to_string(),
            ),
            _ => t_fmt("explorer.status.items", &self.entries.len().to_string()),
        }
    }

    /// 목록 대신 보일 상태 화면이 있으면.
    pub(super) fn find_screen(&self) -> Option<FindScreen> {
        let find = self.find.as_ref()?;
        if find.query.is_empty() {
            return None;
        }
        let folder = find.root.file_name().map_or_else(
            || find.root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        match &find.search {
            Some(search) => match &search.outcome {
                Outcome::Failed(msg) => Some(FindScreen::Failed(msg.clone())),
                Outcome::Done if search.hits.is_empty() => {
                    Some(FindScreen::NoMatches { folder, deep: true })
                }
                _ => None,
            },
            None if !self.entries.is_empty() && self.shown_entries().is_empty() => {
                Some(FindScreen::NoMatches {
                    folder,
                    deep: false,
                })
            }
            None => None,
        }
    }

    /// 하위 폴더 검색 결과를 받는다. 받은 것이 있으면 true.
    pub(super) fn poll_find(&mut self, owner: &mut crate::app::local_reads::ReadRequests) -> bool {
        let sort = self.loaded.as_ref().map(|(_, col, dir)| (*col, *dir));
        let Some(search) = self.find.as_mut().and_then(|f| f.search.as_mut()) else {
            return false;
        };
        let Some(receipt) = search.receipt.as_mut() else {
            return false;
        };
        let events = receipt.poll(owner);
        if events.is_empty() {
            return false;
        }
        for event in events {
            match event {
                SearchEvent::Hits(mut hits) => search.hits.append(&mut hits),
                SearchEvent::Skipped(dir) => search.skipped.push(dir),
                SearchEvent::Failed(msg) => search.outcome = Outcome::Failed(msg),
                SearchEvent::Done { stopped } => {
                    search.outcome = if stopped {
                        Outcome::Stopped
                    } else {
                        Outcome::Done
                    };
                }
            }
        }
        if search.outcome != Outcome::Running {
            search.receipt = None;
        }
        if let Some((col, dir)) = sort {
            sort_entries(&mut search.hits, col, dir);
        }
        true
    }
}

impl FindState {
    /// 하위 폴더 검색이 켜져 있고 검색어가 있으면 처음부터 다시 찾는다. 아니면 검색을 버린다.
    fn restart(&mut self) {
        self.search = (self.deep && !self.query.is_empty()).then(|| Search {
            receipt: Some(crate::app::local_reads::search(
                self.root.clone(),
                self.query.clone(),
            )),
            hits: Vec::new(),
            skipped: Vec::new(),
            outcome: Outcome::Running,
        });
    }
}

/// 시작 폴더 기준 상대 경로. 화면에 보이는 로컬 경로라 OS 구분자를 그대로 쓴다.
fn relative_path(start: &Path, path: &Path) -> String {
    path.strip_prefix(start)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
}

/// 하위 폴더 검색 결과가 든 폴더. 시작 폴더 자신은 ".".
pub(super) fn hit_folder(root: &Path, hit: &Path) -> String {
    crate::app::local_reads::relative_folder(root, hit)
}

/// 이름 글자 배치. 검색어와 맞는 부분을 explorer-match-fg 로 칠한다. 검색어가 없으면 한 색이다.
pub(super) fn name_job(
    theme: &Theme,
    name: &str,
    query: &str,
    font: egui::FontId,
    color: egui::Color32,
    line_height: Option<f32>,
) -> egui::text::LayoutJob {
    let mut job = explorer_match_job(theme, name, query, font, color);
    for section in &mut job.sections {
        section.format.line_height = line_height;
    }
    job
}

impl ExplorerView {
    /// List · Grid 의 하위 폴더 검색 결과는 든 폴더를 툴팁으로 보인다.
    pub(super) fn hit_tooltip(&self, entry: &DirEntryInfo, resp: egui::Response) -> egui::Response {
        match self.search_root() {
            Some(root) => resp.on_hover_text(hit_folder(root, &entry.path)),
            None => resp,
        }
    }
}

/// 툴바 아래 Find 바를 그린다. 원격 explorer 는 Subfolders 를 숨긴다.
pub(super) fn bar(ui: &mut egui::Ui, theme: &Theme, view: &mut ExplorerView, remote: bool) {
    let shown = view.shown_entries().len();
    let total = view.entries.len();
    let Some(find) = view.find.as_mut() else {
        return;
    };
    let folder = find.root.file_name().map_or_else(
        || find.root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let placeholder = if find.deep {
        t_fmt("explorer.find.search_placeholder", &folder)
    } else {
        t("explorer.find.filter_placeholder").to_string()
    };
    let (text, skipped, tooltip) = status_parts(find, shown, total);
    let status = match &find.search {
        Some(search) if search.outcome == Outcome::Running => ExplorerFindStatus::Searching {
            text: &text,
            stop: t("explorer.find.stop"),
        },
        Some(search) if !search.skipped.is_empty() => ExplorerFindStatus::Skipped {
            text: &text,
            skipped: &skipped,
            tooltip: &tooltip,
        },
        _ => ExplorerFindStatus::Text(&text),
    };
    let focus = std::mem::take(&mut find.focus);
    let mut deep = find.deep;
    let events = explorer_find_bar(
        ui,
        theme,
        ExplorerFindBar {
            query: &mut find.query,
            subfolders: (!remote).then_some(&mut deep),
            status,
            labels: ExplorerFindLabels {
                placeholder: &placeholder,
                subfolders: t("explorer.find.subfolders"),
                close: t("explorer.find.close"),
            },
            focus,
        },
    );
    view.apply_find_events(events, deep);
}

impl ExplorerView {
    /// Find 바에서 일어난 일을 반영한다. `deep` 은 바가 돌려준 Subfolders 값이다.
    pub(super) fn apply_find_events(&mut self, events: ExplorerFindEvents, deep: bool) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        find.field_focused = events.field_focused;
        if events.close || (events.escape && find.query.is_empty()) {
            self.find = None;
            return;
        }
        if events.escape {
            // 첫 Esc 는 글자만 지우고 입력을 계속 쓸 수 있게 둔다.
            find.query.clear();
            find.focus = true;
            find.search = None;
            return;
        }
        if events.stop
            && let Some(receipt) = find.search.as_ref().and_then(|s| s.receipt.as_ref())
        {
            receipt.stop();
        }
        if events.subfolders_toggled || events.changed {
            find.deep = deep;
            find.restart();
            self.retain_shown_selection();
        }
    }

    /// 보이지 않게 된 항목을 선택에서 뺀다. 숨은 항목에 명령이 닿지 않게 하기 위해서다.
    fn retain_shown_selection(&mut self) {
        let shown: std::collections::HashSet<PathBuf> =
            self.shown_entries().into_iter().map(|e| e.path).collect();
        self.selected.retain(|p| shown.contains(p));
        if self.anchor.as_ref().is_some_and(|a| !shown.contains(a)) {
            self.anchor = None;
        }
    }
}

/// (상태 글자, 건너뛴 폴더 글자, 건너뛴 폴더 목록 툴팁).
fn status_parts(find: &FindState, shown: usize, total: usize) -> (String, String, String) {
    let Some(search) = &find.search else {
        let text = if find.query.is_empty() {
            String::new()
        } else {
            t_fmt2(
                "explorer.find.count",
                &shown.to_string(),
                &total.to_string(),
            )
        };
        return (text, String::new(), String::new());
    };
    let n = search.hits.len().to_string();
    let text = match search.outcome {
        Outcome::Running => t_fmt("explorer.find.searching", &n),
        Outcome::Done => t_fmt("explorer.find.found", &n),
        Outcome::Stopped => t_fmt("explorer.find.stopped", &n),
        Outcome::Failed(_) => "—".to_string(),
    };
    let skipped = t_fmt("explorer.find.skipped", &search.skipped.len().to_string());
    let tooltip = search
        .skipped
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    (text, skipped, tooltip)
}

#[cfg(test)]
mod tests;
