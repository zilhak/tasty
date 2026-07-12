#![forbid(unsafe_code)]

//! Tasty Git Viewer plugin — read-only git status / log / diff popup (**egui-mesh**).
//!
//! popup contribute (`trigger = ipc`, `rendering = egui-mesh`)로 등록되며, 사이드바 도구
//! 메뉴의 "Git" 항목 클릭이 호스트의 `pending_popup_opens` 경로를 통해 `popup.open` 으로
//! 전달된다. plugin 은 context payload 의 `cwd` 로 git repo 를 탐색해 status/log/diff 를
//! 프로세스 내에서 직접 수집하고(host IPC 없음), 콘텐츠를 자기 egui Context 로 그려
//! mesh 를 host 에 회신한다. host 는 셸(scrim/border/Esc/outside-click)만 소유한다.
//!
//! Theme 은 `popup.set_context` 의 `theme`(ThemeWire)로 매 frame 받아 host 와 동일
//! `Theme` 로 재구성한다(markdown surface 와 동형). 상호작용(worktree 선택 / 파일→diff /
//! Back / Refresh)은 forward 된 실제 사용자 입력으로 egui 안에서 처리된다.

mod git;
mod render;

use std::path::PathBuf;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::{
    Plugin, PluginEnv, PopupClosedCtx, PopupOpenCtx, PopupOpenResult, PopupSetContextCtx,
    SurfaceCreateCtx, SurfaceResult, Translator,
};
use tasty_type_appearance::theme::Theme;

use tasty_plugin_sdk::EguiMeshPopup;

const PLUGIN_ID: &str = "com.tasty.git-viewer";
// Cargo.toml 이 SoT — 하드코딩 드리프트(0.1.8 vs 0.1.10 실재했음)를 컴파일 타임에 차단.
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");
const LOG_LIMIT: usize = 200;

#[derive(Default)]
pub(crate) struct ViewerState {
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
}

struct GitViewerPlugin {
    /// 단일 인스턴스 가드 — 최초 open 이 primary. 이후 인스턴스는 "이미 열림" 표시.
    primary: Option<u64>,
    /// primary 인스턴스의 상태.
    state: Option<ViewerState>,
    /// popup instance_id → egui-mesh 렌더 상태(폰트 atlas·shared buffer 소유).
    popups: HashMap<u64, EguiMeshPopup>,
    /// CJK fallback 폰트를 이미 설치한 popup instance_id.
    fonts_installed: HashSet<u64>,
    tr: Translator,
}

impl GitViewerPlugin {
    fn new(env: &PluginEnv) -> Self {
        Self {
            primary: None,
            state: None,
            popups: HashMap::new(),
            fonts_installed: HashSet::new(),
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

    // popup-only plugin이라 surface 콜백은 빈 결과.
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn open_popup(&mut self, ctx: PopupOpenCtx) -> PopupOpenResult {
        // egui-mesh popup 은 tree 를 안 그린다 — 빈 트리. 최초 인스턴스만 state 를 적재하고
        // primary 로 삼는다. 이후 인스턴스는 paint_popup 에서 "이미 열림" 을 그린다.
        if self.primary.is_none() {
            self.primary = Some(ctx.instance_id);
            let cwd = cwd_from_context(&ctx.context);
            self.state = Some(ViewerState::load(cwd.as_deref()));
        }
        PopupOpenResult::default()
    }

    fn paint_popup(&mut self, ctx: PopupSetContextCtx) {
        self.paint_popup_impl(ctx);
    }

    fn on_popup_closed(&mut self, ctx: PopupClosedCtx) {
        if self.primary == Some(ctx.instance_id) {
            self.primary = None;
            self.state = None;
        }
        self.popups.remove(&ctx.instance_id);
        self.fonts_installed.remove(&ctx.instance_id);
    }
}

impl GitViewerPlugin {
    /// `popup.set_context` 한 frame 을 그려 host 에 popup mesh 를 회신한다.
    fn paint_popup_impl(&mut self, ctx: PopupSetContextCtx) {
        let iid = ctx.params.instance_id;

        // host 가 Theme 을 아직 안 보냈으면 토큰을 풀 수 없으므로 이 frame 건너뜀.
        let Some(theme) = ctx.params.theme.as_ref().map(theme_from_wire) else {
            tracing::debug!("git-viewer popup {iid}: set_context without theme — skipping paint");
            return;
        };

        let is_primary = self.primary == Some(iid);
        // 서로소 필드 — 동시 mutable 차용 안전.
        let tr = &self.tr;
        let state = &mut self.state;
        let is_new = !self.popups.contains_key(&iid);
        let popup = self
            .popups
            .entry(iid)
            .or_insert_with(|| EguiMeshPopup::new(iid));
        if is_new {
            install_fonts(popup.context());
            self.fonts_installed.insert(iid);
        }

        let result = popup.paint(&ctx.host, &ctx.params, |egui_ctx| {
            if is_primary {
                if let Some(st) = state.as_mut() {
                    render::draw(egui_ctx, &theme, st, tr);
                }
            } else {
                render::draw_busy(egui_ctx, &theme, tr);
            }
        });
        if let Err(e) = result {
            tracing::warn!("git-viewer popup {iid} paint failed: {e}");
        }
    }
}

/// wire 스냅샷을 host 와 동일한 `Theme` 인스턴스로 재구성 (sizing 은 zoom 으로 재도출).
fn theme_from_wire(w: &ThemeWire) -> Theme {
    Theme::with_colors_and_zoom(w.colors.clone(), w.is_light, w.ui_zoom)
}

/// plugin Context 에 CJK fallback 을 설치한다(한글/일문/한자 커밋 메시지·경로 tofu 방지).
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = load_system_cjk_font_data() {
        fonts.font_data.insert(
            "system_cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(fam)
                .or_default()
                .push("system_cjk".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

/// 시스템 CJK 폰트 바이트 로드 (host `font_registry::load_system_cjk_font_data` 미러).
fn load_system_cjk_font_data() -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        // host font_registry::load_system_cjk_font_data 미러 (맑은 고딕).
        if let Ok(data) = std::fs::read("C:/Windows/Fonts/malgun.ttf") {
            return Some(data);
        }
    }
    #[cfg(target_os = "macos")]
    {
        for path in &[
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for path in &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }
    None
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
