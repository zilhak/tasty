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

mod render;

use std::path::PathBuf;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;
use tasty_git_core as git;
use tasty_plugin_protocol::ThemeWire;
use tasty_plugin_sdk::{
    BusHandle, EventDispatchCtx, HostHandle, Plugin, PluginEnv, PopupClosedCtx, PopupOpenCtx,
    PopupOpenResult, PopupSetContextCtx, SurfaceCreateCtx, SurfaceResult, Translator,
};
use tasty_type_appearance::theme::Theme;

use tasty_plugin_sdk::EguiMeshPopup;

const PLUGIN_ID: &str = "com.tasty.git-viewer";
// Cargo.toml 이 SoT — 하드코딩 드리프트(0.1.8 vs 0.1.10 실재했음)를 컴파일 타임에 차단.
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");
const LOG_LIMIT: usize = 200;
/// (ADR-0056) host → 이 plugin unicast 이벤트 key. host 측 대응값은
/// `src/app/attach_client.rs::GIT_VIEWER_QUERY_RESULT_EVENT` — 공유 crate 가 없어
/// 리터럴을 양쪽에 중복 정의한다(둘 다 바꿀 때 동기화 필요).
const GIT_VIEWER_QUERY_RESULT_EVENT: &str = "git_viewer.query_result";

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
    /// diff pane 의 가로 스크롤 콘텐츠 폭 캐시 — `(폭을 잰 mono 폰트 크기, 콘텐츠 폭)`.
    /// diff 리스트는 보이는 라인만 그리므로(virtualization), 전 라인의 최장 폭을 한 번
    /// 재 담아두지 않으면 스크롤 위치마다 가로 폭이 출렁인다. render 가 캐시 미스일 때
    /// 채우고, [`ViewerState::set_diff`] 가 diff 를 바꿀 때 비운다.
    diff_width: Option<(f32, f32)>,
    /// (ADR-0056) mirror(attach) surface 에서 열렸으면 Some — 로컬 `git2::Repository`
    /// discover 대신 host 왕복(`git_viewer.query` IPC → `git_viewer.query_result`
    /// event)으로 조회한다. None 이면 기존 로컬 경로(변경 없음).
    remote: Option<RemoteCtx>,
    /// 최초 스냅샷/refresh/worktree 전환 응답이 아직 안 왔다 — render 가 "loading" 을 낸다.
    loading: bool,
    /// 활성 worktree 의 열린 `Repository` 핸들 캐시 — 조작마다 다시 열지 않는다.
    /// 접근은 [`ViewerState::take_repo`] / [`ViewerState::put_repo`] 한 쌍으로만 한다.
    /// 원격(attach) 모드는 로컬 repo 를 열지 않으므로 항상 `None` 이다.
    repo: Option<CachedRepo>,
}

/// 캐시된 `Repository` 핸들과 그 핸들이 바인딩된 workdir.
///
/// `git2::Repository` 는 `Send` 지만 `Sync` 는 아니다(`git2` 의
/// `unsafe impl Send for Repository`). plugin 은 SDK 의 단일 `plugin-worker`
/// 스레드에서 `&mut self` 로만 dispatch 되므로(`tasty-plugin-sdk` 의 `worker_loop`)
/// 이 핸들을 상태에 보관해도 공유가 발생하지 않는다.
struct CachedRepo {
    /// 이 핸들이 가리키는 working dir. 요청 경로와 다르면 캐시 미스로 본다.
    workdir: PathBuf,
    handle: git2::Repository,
}

/// mirror 모드 전용 상태 — host handle + 진행 중인 왕복 요청 추적.
struct RemoteCtx {
    host: HostHandle,
    /// popup 이 anchor 된 **로컬** mirror surface id(`popup.open` context 의
    /// `local_surface_id` echo) — host 가 attach 세션 매핑으로 원격 id 로 치환한다.
    local_surface_id: u32,
    /// 마지막으로 보낸 요청의 id. 응답의 `request_id` 가 다르면 stale 로 버린다(다른
    /// worktree 선택/refresh 로 새 요청이 이미 나간 뒤 도착한 이전 응답 등).
    pending_request_id: Option<u64>,
    /// pending 요청이 diff 조회였다면 그 대상 `status_entries` 인덱스. snapshot 요청이면
    /// None(응답 kind 로 구분하지 않고 이 필드로 분기 — kind 문자열은 host wire 값 그대로).
    pending_diff_idx: Option<usize>,
}

/// `git_viewer.query_result` 이벤트 payload wire — host
/// `attach_client.rs::apply_attach_client_output` 의 `MirrorEvent::GitQueryResult` unicast
/// 조립과 대칭.
#[derive(serde::Deserialize)]
struct GitQueryReplyWire {
    request_id: u64,
    ok: bool,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    data: Option<Value>,
    /// 서버가 payload 예산(700KiB) 초과로 status/log/diff 일부를 잘랐는가 — 잘려도
    /// `data` 자체는 유효하므로 현재는 UI 배너 없이 무시한다(list_dir 의 toast 같은
    /// 별도 채널이 popup 안엔 없음).
    #[serde(default)]
    #[allow(dead_code)]
    truncated: bool,
    #[serde(default)]
    reason: Option<String>,
}

/// snapshot 응답 `data` wire — 필드명이 `tasty_git_core::WorktreeEntry`/`StatusEntry`/
/// `LogEntry` 와 그대로 일치해 host `attach_runtime.rs::git_query_snapshot` 의 wire
/// 조립과 대칭(별도 DTO 중복 없음).
#[derive(serde::Deserialize)]
struct SnapshotWire {
    active_worktree_path: String,
    worktrees: Vec<git::WorktreeEntry>,
    status_entries: Vec<git::StatusEntry>,
    log_entries: Vec<git::LogEntry>,
}

impl ViewerState {
    /// mirror(attach) surface 용 초기 상태 — 즉시 첫 스냅샷 요청을 보내고 loading 상태로
    /// 시작한다. 로컬 `load`(discover_repo 직접 호출)와 대칭.
    fn new_remote(host: HostHandle, local_surface_id: u32) -> Self {
        let mut s = ViewerState {
            remote: Some(RemoteCtx {
                host,
                local_surface_id,
                pending_request_id: None,
                pending_diff_idx: None,
            }),
            ..Default::default()
        };
        s.request_remote_snapshot(None);
        s
    }

    /// host 에 `git_viewer.query{kind:snapshot}` 를 보낸다. `worktree_path` 는 이전
    /// 응답이 돌려준 opaque 서버 경로 echo(worktree 전환/refresh) — None 이면 서버가
    /// mirror surface 의 원격 cwd 로 새로 discover 한다(최초 로드).
    fn request_remote_snapshot(&mut self, worktree_path: Option<String>) {
        let Some(remote) = self.remote.as_mut() else {
            return;
        };
        let params = serde_json::json!({
            "kind": "snapshot",
            "local_surface_id": remote.local_surface_id,
            "worktree_path": worktree_path,
        });
        match remote.host.call("git_viewer.query", params) {
            Ok(v) => {
                remote.pending_request_id = v.get("request_id").and_then(Value::as_u64);
                remote.pending_diff_idx = None;
                self.loading = true;
                self.error = None;
            }
            Err(e) => {
                tracing::warn!("git_viewer.query(snapshot) failed: {e}");
                self.error = Some(e.to_string());
                self.loading = false;
            }
        }
    }

    /// host 에 `git_viewer.query{kind:diff}` 를 보낸다. `idx` 는 `status_entries` 내
    /// 대상 인덱스(응답 적용 시 `pending_diff_idx` 로 재확인해 stale reply 를 거른다).
    fn request_remote_diff(&mut self, idx: usize, diff_path: String) {
        let worktree_path = self
            .worktrees
            .get(self.active_worktree)
            .map(|w| w.path.to_string_lossy().into_owned());
        let Some(remote) = self.remote.as_mut() else {
            return;
        };
        let params = serde_json::json!({
            "kind": "diff",
            "local_surface_id": remote.local_surface_id,
            "worktree_path": worktree_path,
            "diff_path": diff_path,
        });
        match remote.host.call("git_viewer.query", params) {
            Ok(v) => {
                remote.pending_request_id = v.get("request_id").and_then(Value::as_u64);
                remote.pending_diff_idx = Some(idx);
                self.error = None;
            }
            Err(e) => {
                tracing::warn!("git_viewer.query(diff) failed: {e}");
                self.error = Some(e.to_string());
            }
        }
    }

    /// `on_event` 가 `git_viewer.query_result` 를 받을 때마다 호출 — `request_id` 가
    /// 현재 pending 과 다르면 stale 응답으로 조용히 버린다.
    fn apply_remote_reply(&mut self, reply: GitQueryReplyWire) {
        let Some(remote) = self.remote.as_mut() else {
            return;
        };
        if !should_apply_remote_reply(remote.pending_request_id, reply.request_id) {
            return;
        }
        let pending_diff_idx = remote.pending_diff_idx.take();
        remote.pending_request_id = None;
        self.loading = false;

        if !reply.ok {
            self.error = Some(
                reply
                    .reason
                    .unwrap_or_else(|| "remote git query failed".to_string()),
            );
            return;
        }
        self.error = None;
        let Some(data) = reply.data else {
            self.error = Some("remote git query returned no data".to_string());
            return;
        };
        match reply.kind.as_str() {
            "snapshot" => self.apply_remote_snapshot(data),
            "diff" => {
                if let Some(idx) = pending_diff_idx {
                    self.apply_remote_diff(idx, data);
                }
            }
            other => tracing::warn!("git-viewer: unknown git_viewer.query_result kind '{other}'"),
        }
    }

    fn apply_remote_snapshot(&mut self, data: Value) {
        let wire: SnapshotWire = match serde_json::from_value(data) {
            Ok(w) => w,
            Err(e) => {
                self.error = Some(format!("malformed snapshot reply: {e}"));
                return;
            }
        };
        let active_path = PathBuf::from(&wire.active_worktree_path);
        // 최초 스냅샷에서만 고정 — 로컬 `load`(current_workdir 는 popup open 시점 1회
        // 설정)와 동일 불변식. worktree 전환/refresh 로는 갱신하지 않아 `is_current`
        // 배지가 항상 "popup 을 연 위치" 를 가리킨다(선택 중인 worktree 와 별개).
        if self.current_workdir.is_none() {
            self.current_workdir = Some(active_path.clone());
        }
        self.repo_path = Some(active_path.clone());
        let mut worktrees = wire.worktrees;
        if let Some(pinned) = &self.current_workdir {
            for w in &mut worktrees {
                w.is_current = &w.path == pinned;
            }
        }
        self.active_worktree = worktrees
            .iter()
            .position(|w| w.path == active_path)
            .unwrap_or(0);
        self.worktrees = worktrees;
        self.status_entries = wire.status_entries;
        self.log_entries = wire.log_entries;
        // diff 뷰는 원격 재조회가 필요해 snapshot 갱신에 자동으로 딸려 오지 않는다 —
        // 선택된 파일이 새 목록에 없을 수도 있으므로 단순하게 닫는다(로컬처럼 같은
        // 인덱스를 이어서 재요청하지 않음 — 흔치 않은 edge case 라 단순함을 우선).
        self.selected_file = None;
        self.set_diff(None);
    }

    fn apply_remote_diff(&mut self, idx: usize, data: Value) {
        if self.selected_file != Some(idx) {
            // 사용자가 이미 다른 파일을 선택했거나 Back 으로 diff 를 닫음 — stale.
            return;
        }
        match serde_json::from_value::<git::DiffData>(data) {
            Ok(diff) => self.set_diff(Some(diff)),
            Err(e) => self.error = Some(format!("malformed diff reply: {e}")),
        }
    }

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

        // 활성 worktree 가 popup cwd 의 worktree(= 방금 연 `repo`)면 그 핸들을 그대로
        // 쓴다 — 같은 repo 를 다시 열 이유가 없다. `is_current` 가 아닌 경우(cwd 의
        // worktree 를 목록에서 못 찾아 0번으로 폴백)만 `bind_active` 가 대상 worktree 를
        // 새로 연다.
        let active_is_current = s
            .worktrees
            .get(s.active_worktree)
            .is_some_and(|w| w.is_current);
        if s.worktrees.is_empty() || active_is_current {
            // worktree 도출 실패(빈 목록)면 단일 repo 흐름으로 폴백하고, 활성이 곧
            // 이 repo 면 그대로 바인딩한다 — 양쪽 다 `repo` 를 그대로 쓴다.
            s.repo_path = Some(current_wd.clone());
            s.refresh_collections(&repo);
            s.put_repo(current_wd, repo);
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
        // worktree 가 바뀌면 `path` 가 캐시 키와 달라 자동으로 미스가 나고, 이전
        // worktree 의 핸들은 그 자리에서 버려진다.
        let Some(repo) = self.take_repo(&path) else {
            self.error = Some(format!("repo lost at {}", path.display()));
            return;
        };
        self.repo_path = repo
            .workdir()
            .map(|p| p.to_path_buf())
            .or(Some(path.clone()));
        self.refresh_collections(&repo);
        self.put_repo(self.repo_path.clone().unwrap_or(path), repo);
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
        // `entry` 대여를 여기서 끝낸다 — 아래 `set_diff` 가 `&mut self` 를 잡는다.
        let path = entry.path.to_string_lossy().into_owned();
        self.selected_file = None;
        self.set_diff(None);
        self.error = None;
        // 다른 worktree 로 간다 — 이전 worktree 의 핸들은 여기서 버린다(로컬 경로의
        // `bind_active` 도 키 불일치로 미스가 나지만, 원격 경로는 아래에서 바로
        // 돌아가므로 명시적으로 비워 낡은 핸들이 남지 않게 한다).
        self.repo = None;
        if self.remote.is_some() {
            self.active_worktree = idx;
            self.request_remote_snapshot(Some(path));
            return;
        }
        self.active_worktree = idx;
        self.bind_active();
    }

    fn refresh(&mut self) {
        if self.remote.is_some() {
            let worktree_path = self
                .worktrees
                .get(self.active_worktree)
                .map(|w| w.path.to_string_lossy().into_owned());
            self.request_remote_snapshot(worktree_path);
            return;
        }
        // Refresh 는 "지금 상태를 다시 읽어달라" 는 명시적 요청이다. 캐시된 핸들을
        // 여기서 **먼저 버려** 재조회가 항상 새로 연 repo 에서 이뤄지게 한다 —
        // 최신성이 캐시 적중률보다 우선이므로, 외부 변경 반영 여부가 핸들 수명에
        // 좌우되지 않는다.
        self.repo = None;

        // worktree 목록 재수집(외부 add/remove 반영) — current_workdir 기준.
        if let Some(current_wd) = self.current_workdir.clone() {
            let prev_active = self
                .worktrees
                .get(self.active_worktree)
                .map(|e| e.path.clone());
            if let Some(repo) = git::discover_repo(&current_wd) {
                if let Ok(v) = git::collect_worktrees(&repo, &current_wd)
                    && !v.is_empty()
                {
                    self.worktrees = v;
                    // 목록이 바뀌었을 수 있으니 이전 활성 경로로 인덱스 보정.
                    self.active_worktree = prev_active
                        .and_then(|p| self.worktrees.iter().position(|e| e.path == p))
                        .or_else(|| self.worktrees.iter().position(|w| w.is_current))
                        .unwrap_or(0);
                }
                // 활성 worktree 가 방금 연 그 repo 라면 핸들을 넘겨 아래 재바인딩에서
                // 재사용한다 — 한 번의 Refresh 안에서 같은 repo 를 두 번 열지 않는다.
                if self.repo_path.as_deref() == Some(current_wd.as_path()) {
                    self.put_repo(current_wd, repo);
                }
            }
        }

        let Some(path) = self.repo_path.clone() else {
            return;
        };
        let Some(repo) = self.take_repo(&path) else {
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
                self.set_diff(None);
            }
        }
        self.put_repo(path, repo);
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
        if self.remote.is_some() {
            self.selected_file = Some(idx);
            self.request_remote_diff(idx, entry.path);
            return;
        }
        let Some(path) = self.repo_path.clone() else {
            return;
        };
        let Some(repo) = self.take_repo(&path) else {
            return;
        };
        self.selected_file = Some(idx);
        self.load_diff_for(&repo, &entry.path);
        self.put_repo(path, repo);
    }

    fn load_diff_for(&mut self, repo: &git2::Repository, path: &str) {
        match git::collect_diff(repo, path) {
            Ok(d) => self.set_diff(Some(d)),
            Err(e) => {
                tracing::warn!("collect_diff failed: {e}");
                self.error = Some(e.to_string());
                self.set_diff(None);
            }
        }
    }

    fn close_diff(&mut self) {
        self.selected_file = None;
        self.set_diff(None);
    }

    /// `diff_content` 는 반드시 이 헬퍼로만 바꾼다 — 같이 무효화해야 하는
    /// [`ViewerState::diff_width`] 캐시가 딸려 있다.
    fn set_diff(&mut self, diff: Option<git::DiffData>) {
        self.diff_content = diff;
        self.diff_width = None;
    }

    /// `workdir` 에 해당하는 `Repository` 를 꺼낸다 — 캐시가 같은 workdir 이면 그
    /// 핸들을, 아니면 새로 열어 돌려준다(다른 worktree 의 캐시는 여기서 버려진다).
    ///
    /// **캐시는 항상 여기서 비워진다.** 호출자가 다 쓴 뒤 [`Self::put_repo`] 로
    /// 돌려놓아야 다음 호출이 재사용하고, 에러로 중간에 빠져나가면 캐시가 빈 채로
    /// 남아 다음 조작이 무조건 다시 연다 — 낡은 핸들이 살아남는 경로가 없다.
    fn take_repo(&mut self, workdir: &std::path::Path) -> Option<git2::Repository> {
        if let Some(cached) = self.repo.take()
            && cached.workdir == workdir
        {
            return Some(cached.handle);
        }
        git::discover_repo(workdir)
    }

    /// [`Self::take_repo`] 로 꺼낸 핸들을 캐시에 돌려놓는다.
    fn put_repo(&mut self, workdir: PathBuf, handle: git2::Repository) {
        self.repo = Some(CachedRepo { workdir, handle });
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
    /// (ADR-0056) `on_start` 에서 1 회 수신 — mirror popup 이 원격 git 조회를 트리거할 때
    /// `ViewerState::new_remote` 에 clone 해 넘긴다.
    host: Option<HostHandle>,
}

impl GitViewerPlugin {
    fn new(env: &PluginEnv) -> Self {
        Self {
            primary: None,
            state: None,
            popups: HashMap::new(),
            fonts_installed: HashSet::new(),
            tr: Translator::from_plugin_env(env),
            host: None,
        }
    }
}

fn cwd_from_context(context: &Value) -> Option<PathBuf> {
    context
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
}

/// `apply_remote_reply` 의 request_id 매칭 판정 — `RemoteCtx`(HostHandle 보유라 이
/// crate 밖에서 생성 불가) 없이 순수 값만으로 단위 테스트할 수 있게 뽑아냈다.
/// `reply_request_id == 0` 은 host 가 mirror workspace disconnect 시 쓰는 sentinel
/// (`request_id` 실제 발급은 1부터 시작 — `App::notify_git_viewer_mirror_lost`) —
/// "지금 뭔가 기다리고 있다면(=`pending_request_id.is_some()`) 무조건 버려라" 로
/// 해석한다. 일반 id 는 정확히 일치해야 한다(stale/중복 응답 거부).
fn should_apply_remote_reply(pending_request_id: Option<u64>, reply_request_id: u64) -> bool {
    if reply_request_id == 0 {
        pending_request_id.is_some()
    } else {
        pending_request_id == Some(reply_request_id)
    }
}

#[cfg(test)]
mod remote_reply_tests {
    use super::*;

    #[test]
    fn sentinel_abandons_when_pending() {
        assert!(should_apply_remote_reply(Some(7), 0));
    }

    #[test]
    fn sentinel_ignored_when_idle() {
        assert!(!should_apply_remote_reply(None, 0));
    }

    #[test]
    fn normal_id_must_match_exactly() {
        assert!(should_apply_remote_reply(Some(7), 7));
        assert!(!should_apply_remote_reply(Some(7), 8));
        assert!(!should_apply_remote_reply(None, 7));
    }
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
            // (ADR-0056) mirror workspace 면 로컬 discover 대신 host 왕복으로 조회한다
            // (`tools_menu.rs` 가 context 에 `mirror`/`local_surface_id` 를 실어 보냄).
            let is_mirror = ctx
                .context
                .get("mirror")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let local_surface_id = ctx
                .context
                .get("local_surface_id")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
            self.state = match (is_mirror, local_surface_id, self.host.clone()) {
                (true, Some(sid), Some(host)) => Some(ViewerState::new_remote(host, sid)),
                _ => {
                    let cwd = cwd_from_context(&ctx.context);
                    Some(ViewerState::load(cwd.as_deref()))
                }
            };
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

    fn on_event(&mut self, ctx: EventDispatchCtx) {
        if ctx.envelope.key != GIT_VIEWER_QUERY_RESULT_EVENT {
            return;
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match serde_json::from_value::<GitQueryReplyWire>(ctx.envelope.payload) {
            Ok(reply) => state.apply_remote_reply(reply),
            Err(e) => tracing::warn!("git-viewer: malformed git_viewer.query_result event: {e}"),
        }
    }

    fn on_start(&mut self, host: HostHandle, _bus: BusHandle) {
        self.host = Some(host);
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
    // 언어팩 `[font]` 폰트를 CJK 뒤, 체인 맨 뒤 폴백으로 붙인다. host 두 경로와 같은
    // 판정기(`tasty_egui_theme::install_locale_font_fallback`)를 쓴다 — 검증이 곧 "어떤
    // 폰트를 거부하는가" 라는 판정이라 사본을 두면 host 는 받고 plugin 은 거부하는 갈림이
    // 생긴다. 경로는 host 가 resolve 해 `TASTY_LOCALE_FONT` 로 물려준 것(SDK
    // `PluginEnv.locale_font` 와 같은 출처).
    if let Some(path) = std::env::var_os("TASTY_LOCALE_FONT").filter(|v| !v.is_empty()) {
        let path = std::path::PathBuf::from(path);
        if let Err(e) = tasty_egui_theme::install_locale_font_fallback(&mut fonts, &path) {
            tracing::warn!(
                "locale font at {} could not be installed: {e}",
                path.display()
            );
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
