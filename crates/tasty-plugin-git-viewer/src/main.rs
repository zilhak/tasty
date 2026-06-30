#![forbid(unsafe_code)]

//! Tasty Git Viewer plugin — read-only git status / log / diff popup.
//!
//! popup contribute (`trigger = ipc`)로 등록되며, 사이드바 도구 메뉴의 "Git" 항목 클릭이
//! 호스트의 `pending_popup_opens` 경로를 통해 `popup.open` IPC로 전달된다. context payload의
//! `cwd` 필드를 받아 git repo를 탐색하고 status/log/diff를 plugin process 내에서 직접
//! 수집한다 (host IPC 호출 없음).

mod git;
mod view;

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;
use tasty_plugin_sdk::{
    BusHandle, HostHandle, Plugin, PluginEnv, PopupClosedCtx, PopupEventCtx, PopupEventResult,
    PopupOpenCtx, PopupOpenResult, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult, Translator,
    UiEvent,
};

const PLUGIN_ID: &str = "com.tasty.git-viewer";
const PLUGIN_VERSION: &str = "0.1.1";
const LOG_LIMIT: usize = 200;

const ID_REFRESH: &str = "refresh";
const ID_BACK: &str = "back";
const FILE_PREFIX: &str = "file.";
const WT_PREFIX: &str = "wt.";

#[derive(Default)]
struct ViewerState {
    /// 현재 **활성** worktree 의 workdir (status/log/diff 가 바인딩된 대상).
    repo_path: Option<PathBuf>,
    /// popup 이 받은 cwd 가 속한 worktree 의 workdir — `is_current` 판정용(불변).
    current_workdir: Option<PathBuf>,
    /// main + 모든 linked worktree 종합 목록.
    worktrees: Vec<git::WorktreeEntry>,
    /// `worktrees` 내 활성 항목 인덱스.
    active_worktree: usize,
    error: Option<String>,
    status_entries: Vec<git::StatusEntry>,
    log_entries: Vec<git::LogEntry>,
    selected_file: Option<usize>,
    diff_content: Option<git::DiffData>,
}

impl ViewerState {
    fn load(cwd: Option<&std::path::Path>) -> Self {
        let mut s = ViewerState::default();
        let Some(cwd) = cwd else {
            return s;
        };
        let Some(repo) = git::discover_repo(cwd) else {
            return s;
        };
        // popup cwd 의 worktree workdir — is_current 의 기준점(이후 고정).
        let current_wd = repo
            .workdir()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| repo.path().to_path_buf());
        s.current_workdir = Some(current_wd.clone());
        s.worktrees = git::collect_worktrees(&repo, &current_wd).unwrap_or_default();
        s.active_worktree = s.worktrees.iter().position(|w| w.is_current).unwrap_or(0);

        if s.worktrees.is_empty() {
            // worktree 도출 실패 등 — 기존 단일 repo 흐름으로 폴백.
            s.repo_path = Some(current_wd);
            s.refresh_collections(&repo);
        } else {
            s.bind_active();
        }
        s
    }

    /// `active_worktree` 가 가리키는 worktree 로 status/log 컬렉션을 재바인딩한다.
    fn bind_active(&mut self) {
        let Some(path) = self
            .worktrees
            .get(self.active_worktree)
            .map(|e| e.path.clone())
            .or_else(|| self.repo_path.clone())
        else {
            return;
        };
        let Some(repo) = git::discover_repo(&path) else {
            self.error = Some(format!("repo lost at {}", path.display()));
            return;
        };
        self.repo_path = repo.workdir().map(|p| p.to_path_buf()).or(Some(path));
        self.refresh_collections(&repo);
    }

    /// worktree 선택 — 활성 worktree 를 바꾸고 status/log/diff 를 재바인딩(읽기 전용).
    /// 실제 checkout/working dir 변경 없음. invalid worktree 는 전환하지 않는다.
    fn select_worktree(&mut self, idx: usize) {
        let Some(entry) = self.worktrees.get(idx) else {
            return;
        };
        if !entry.is_valid || idx == self.active_worktree {
            return;
        }
        self.active_worktree = idx;
        self.selected_file = None;
        self.diff_content = None;
        self.error = None;
        self.bind_active();
    }

    fn refresh(&mut self) {
        // worktree 목록 재수집(외부 add/remove 반영) — current_workdir 기준.
        if let Some(current_wd) = self.current_workdir.clone() {
            let prev_active = self
                .worktrees
                .get(self.active_worktree)
                .map(|e| e.path.clone());
            if let Some(repo) = git::discover_repo(&current_wd)
                && let Ok(v) = git::collect_worktrees(&repo, &current_wd)
                && !v.is_empty()
            {
                self.worktrees = v;
                // 목록이 바뀌었을 수 있으니 이전 활성 경로로 인덱스 보정.
                self.active_worktree = prev_active
                    .and_then(|p| self.worktrees.iter().position(|e| e.path == p))
                    .or_else(|| self.worktrees.iter().position(|w| w.is_current))
                    .unwrap_or(0);
            }
        }

        let Some(path) = self.repo_path.clone() else {
            return;
        };
        let Some(repo) = git::discover_repo(&path) else {
            self.error = Some(format!("repo lost at {}", path.display()));
            return;
        };
        self.error = None;
        self.refresh_collections(&repo);
        if let Some(idx) = self.selected_file {
            if let Some(entry) = self.status_entries.get(idx).cloned() {
                self.load_diff_for(&repo, &entry.path);
            } else {
                self.selected_file = None;
                self.diff_content = None;
            }
        }
    }

    fn refresh_collections(&mut self, repo: &git2::Repository) {
        match git::collect_status(repo) {
            Ok(v) => self.status_entries = v,
            Err(e) => {
                tracing::warn!("collect_status failed: {e}");
                self.error = Some(e.to_string());
            }
        }
        match git::collect_log(repo, LOG_LIMIT) {
            Ok(v) => self.log_entries = v,
            Err(e) => {
                tracing::warn!("collect_log failed: {e}");
                self.error = Some(e.to_string());
            }
        }
    }

    fn load_diff(&mut self, idx: usize) {
        let Some(entry) = self.status_entries.get(idx).cloned() else {
            return;
        };
        let Some(path) = self.repo_path.clone() else {
            return;
        };
        let Some(repo) = git::discover_repo(&path) else {
            return;
        };
        self.selected_file = Some(idx);
        self.load_diff_for(&repo, &entry.path);
    }

    fn load_diff_for(&mut self, repo: &git2::Repository, path: &str) {
        match git::collect_diff(repo, path) {
            Ok(d) => self.diff_content = Some(d),
            Err(e) => {
                tracing::warn!("collect_diff failed: {e}");
                self.error = Some(e.to_string());
                self.diff_content = None;
            }
        }
    }

    fn close_diff(&mut self) {
        self.selected_file = None;
        self.diff_content = None;
    }

    fn build_tree(&self, tr: &Translator) -> tasty_plugin_sdk::UiNode {
        let vm = view::ViewModel {
            repo_path: self
                .repo_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            error: self.error.as_deref(),
            worktrees: &self.worktrees,
            active_worktree: self.active_worktree,
            status_entries: &self.status_entries,
            log_entries: &self.log_entries,
            selected_file: self.selected_file,
            diff_content: self.diff_content.as_ref(),
        };
        view::main_tree(&vm, tr)
    }
}

struct GitViewerPlugin {
    /// 단일 인스턴스 가드 — 두 번째 open 시 placeholder만 반환.
    current_instance: Mutex<Option<u64>>,
    /// 활성 인스턴스의 상태.
    state: Mutex<Option<ViewerState>>,
    tr: Translator,
}

impl GitViewerPlugin {
    fn new(env: &PluginEnv) -> Self {
        Self {
            current_instance: Mutex::new(None),
            state: Mutex::new(None),
            tr: Translator::from_plugin_env(env),
        }
    }
}

fn cwd_from_context(context: &Value) -> Option<PathBuf> {
    context
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
}

impl Plugin for GitViewerPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn on_start(&mut self, _host: HostHandle, _bus: BusHandle) {}

    // popup-only plugin이라 surface 콜백은 빈 결과.
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn open_popup(&mut self, ctx: PopupOpenCtx) -> PopupOpenResult {
        let mut guard = match self.current_instance.lock() {
            Ok(g) => g,
            Err(_) => return PopupOpenResult { tree: None },
        };
        if guard.is_some() {
            return PopupOpenResult {
                tree: Some(view::already_open_tree(&self.tr)),
            };
        }
        let cwd = cwd_from_context(&ctx.context);
        let new_state = ViewerState::load(cwd.as_deref());
        let tree = new_state.build_tree(&self.tr);

        *guard = Some(ctx.instance_id);
        if let Ok(mut s) = self.state.lock() {
            *s = Some(new_state);
        }
        PopupOpenResult { tree: Some(tree) }
    }

    fn handle_popup_event(&mut self, ctx: PopupEventCtx) -> PopupEventResult {
        let UiEvent::Click { node_id } = &ctx.event else {
            return PopupEventResult {
                tree: None,
                close: false,
            };
        };

        // 주(主) 인스턴스가 아닌 인스턴스(중복 placeholder)에서 온 클릭은 무시.
        if self.current_instance.lock().ok().and_then(|g| *g) != Some(ctx.instance_id) {
            return PopupEventResult {
                tree: None,
                close: false,
            };
        }

        let mut state_guard = match self.state.lock() {
            Ok(g) => g,
            Err(_) => {
                return PopupEventResult {
                    tree: None,
                    close: false,
                };
            }
        };
        let Some(state) = state_guard.as_mut() else {
            return PopupEventResult {
                tree: None,
                close: false,
            };
        };

        let dirty = if node_id == ID_REFRESH {
            state.refresh();
            true
        } else if node_id == ID_BACK {
            state.close_diff();
            true
        } else if let Some(rest) = node_id.strip_prefix(WT_PREFIX) {
            if let Ok(idx) = rest.parse::<usize>() {
                state.select_worktree(idx);
                true
            } else {
                false
            }
        } else if let Some(rest) = node_id.strip_prefix(FILE_PREFIX) {
            if let Ok(idx) = rest.parse::<usize>() {
                state.load_diff(idx);
                true
            } else {
                false
            }
        } else {
            false
        };

        if dirty {
            PopupEventResult {
                tree: Some(state.build_tree(&self.tr)),
                close: false,
            }
        } else {
            PopupEventResult {
                tree: None,
                close: false,
            }
        }
    }

    fn on_popup_closed(&mut self, ctx: PopupClosedCtx) {
        if let Ok(mut g) = self.current_instance.lock()
            && *g == Some(ctx.instance_id)
        {
            *g = None;
            if let Ok(mut s) = self.state.lock() {
                *s = None;
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let env = PluginEnv::load()?;
    let plugin = GitViewerPlugin::new(&env);
    tasty_plugin_sdk::run(plugin)
}
