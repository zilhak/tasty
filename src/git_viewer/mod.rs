//! Git viewer popup — read-only git status / log / diff 표시.
//!
//! 결정 사항: read-only 만 (mutate 작업 없음), 그래프 없음 (Phase 1 평면 리스트),
//! diff 는 working tree vs HEAD 통합. IPC 미노출 — 사용자 UI 편의 기능.

pub mod data;
pub mod diff_panel;
pub mod log_panel;
pub mod status_panel;

use std::path::PathBuf;

pub use data::{DiffData, LogEntry, StatusEntry};

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::PopupAction;

pub const GIT_VIEWER_POPUP_ID: &str = "git_viewer";

/// 한 번에 가져오는 커밋 최대 개수.
const LOG_LIMIT: usize = 200;

#[derive(Debug, Default)]
pub struct GitViewerState {
    pub repo_path: Option<PathBuf>,
    pub error: Option<String>,
    pub status_entries: Vec<StatusEntry>,
    pub log_entries: Vec<LogEntry>,
    pub selected_file: Option<usize>,
    pub diff_content: Option<DiffData>,
}

impl GitViewerState {
    /// popup 이 열릴 때 호출. cwd 에서 repo 를 찾고 status / log 수집.
    pub fn load(cwd: Option<&std::path::Path>) -> Self {
        let mut state = GitViewerState::default();
        let Some(cwd) = cwd else {
            return state;
        };
        let Some(repo) = data::discover_repo(cwd) else {
            return state;
        };
        state.repo_path = repo.path().parent().map(|p| p.to_path_buf());
        match data::collect_status(&repo) {
            Ok(entries) => state.status_entries = entries,
            Err(e) => {
                tracing::warn!("git_viewer: collect_status failed: {e}");
                state.error = Some(e.to_string());
            }
        }
        match data::collect_log(&repo, LOG_LIMIT) {
            Ok(entries) => state.log_entries = entries,
            Err(e) => {
                tracing::warn!("git_viewer: collect_log failed: {e}");
                state.error = Some(e.to_string());
            }
        }
        state
    }

    /// 새로고침. repo_path 가 있으면 상태/로그 다시 수집.
    pub fn refresh(&mut self) {
        let Some(path) = self.repo_path.clone() else {
            return;
        };
        let Some(repo) = data::discover_repo(&path) else {
            self.error = Some(format!("repo lost at {}", path.display()));
            return;
        };
        self.error = None;
        match data::collect_status(&repo) {
            Ok(entries) => self.status_entries = entries,
            Err(e) => {
                tracing::warn!("git_viewer: refresh status failed: {e}");
                self.error = Some(e.to_string());
            }
        }
        match data::collect_log(&repo, LOG_LIMIT) {
            Ok(entries) => self.log_entries = entries,
            Err(e) => {
                tracing::warn!("git_viewer: refresh log failed: {e}");
                self.error = Some(e.to_string());
            }
        }
        // diff 도 새로고침
        if let Some(idx) = self.selected_file {
            if let Some(entry) = self.status_entries.get(idx).cloned() {
                self.load_diff_for(&repo, &entry.path);
            } else {
                self.selected_file = None;
                self.diff_content = None;
            }
        }
    }

    /// 특정 파일 diff 를 수집해서 `diff_content` 에 저장.
    pub fn load_diff(&mut self, idx: usize) {
        let Some(entry) = self.status_entries.get(idx).cloned() else {
            return;
        };
        let Some(path) = self.repo_path.clone() else {
            return;
        };
        let Some(repo) = data::discover_repo(&path) else {
            return;
        };
        self.selected_file = Some(idx);
        self.load_diff_for(&repo, &entry.path);
    }

    fn load_diff_for(&mut self, repo: &git2::Repository, path: &str) {
        match data::collect_diff(repo, path) {
            Ok(d) => self.diff_content = Some(d),
            Err(e) => {
                tracing::warn!("git_viewer: collect_diff failed: {e}");
                self.error = Some(e.to_string());
                self.diff_content = None;
            }
        }
    }

    pub fn close_diff(&mut self) {
        self.selected_file = None;
        self.diff_content = None;
    }
}

/// popup 본체. PopupAction::Close 면 호스트가 popup 을 닫는다.
pub fn draw_git_viewer_popup(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    // ESC 닫기
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    // popup 열려 있는 동안 dialogs 에 state 가 있어야 한다. 없으면 첫 진입.
    if state.dialogs.git_viewer.is_none() {
        let cwd = state.resolve_inherit_cwd();
        state.dialogs.git_viewer = Some(GitViewerState::load(cwd.as_deref()));
    }

    let th = theme::theme();
    let gv = state.dialogs.git_viewer.as_mut().expect("just initialized");

    // 헤더
    ui.horizontal(|ui| {
        if ui.small_button(t("git_viewer.refresh")).clicked() {
            gv.refresh();
        }
        if let Some(p) = &gv.repo_path {
            ui.label(
                egui::RichText::new(format!("({})", p.display()))
                    .small()
                    .color(th.subtext0),
            );
        }
    });
    if let Some(err) = &gv.error {
        ui.label(
            egui::RichText::new(t("git_viewer.error").replace("{0}", err))
                .small()
                .color(th.red),
        );
    }
    ui.separator();

    // repo 없음
    if gv.repo_path.is_none() {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new(t("git_viewer.no_repo"))
                    .color(th.subtext0),
            );
        });
        return PopupAction::None;
    }

    let available = ui.available_height();
    let panel_h = (available - 8.0) / 2.0; // 5:5 분할, 분리선 여백 8px

    // 상단: status
    let mut clicked_file: Option<usize> = None;
    egui::Frame::new().show(ui, |ui| {
        ui.set_min_height(panel_h);
        ui.set_max_height(panel_h);
        ui.label(
            egui::RichText::new(format!(
                "{} ({})",
                t("git_viewer.status_heading"),
                gv.status_entries.len()
            ))
            .strong()
            .small(),
        );
        clicked_file = status_panel::draw_status_panel(ui, gv);
    });
    if let Some(idx) = clicked_file {
        gv.load_diff(idx);
    }

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    // 하단: diff or log
    let mut close_diff = false;
    egui::Frame::new().show(ui, |ui| {
        ui.set_min_height(panel_h);
        if gv.selected_file.is_some() {
            close_diff = diff_panel::draw_diff_panel(ui, gv);
        } else {
            ui.label(
                egui::RichText::new(t("git_viewer.log_heading"))
                    .strong()
                    .small(),
            );
            log_panel::draw_log_panel(ui, gv);
        }
    });
    if close_diff {
        gv.close_diff();
    }

    PopupAction::None
}
