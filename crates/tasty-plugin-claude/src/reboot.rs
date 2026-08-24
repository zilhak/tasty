//! `claude.reboot` — surface 안의 claude 를 종료하고 같은 세션으로 재기동한다.
//!
//! claude code 는 스스로 자기 TUI 를 껐다 켤 수 없으므로, 에이전트가
//! `tasty claude reboot` 를 호출하면 plugin 이 대신 수행한다:
//! 지연(기본 5s) → Ctrl+C ×4(0.5s 간격) → 셸에 `claude -r <session_id>` 전송 →
//! TUI 복귀 확인 후 재시작 안내 프롬프트를 `terminal.tell` 로 제출.
//!
//! session id 는 **요청 시점에** surface meta(`claude-session-id`, session-start
//! hook 이 기록)에서 캡처한다 — Ctrl+C 종료가 session-end hook 을 발화시키면
//! meta 가 지워지기 때문. 안내 프롬프트는 셸 인자가 아니라 `terminal.tell` 로
//! 보낸다 — Windows 기본 셸(cmd.exe)에는 `"$(cat …)"` 전달 패턴이 없기 때문.
//!
//! 안전 가드: 전경 프로세스 이름을 요청 시점에 baseline 으로 캡처해 두고,
//! Ctrl+C 후에도 전경이 baseline(=claude)이면 **아무 텍스트도 보내지 않고 중단**
//! 한다(살아있는 TUI 입력창 오염 방지). resume 후 전경이 baseline 으로 복귀하지
//! 않으면 안내 프롬프트도 보내지 않는다(셸에 평문이 명령으로 실행되는 사고 방지).
//!
//! **Claude 세션 프로필** — `--profile-file <경로>` 를 주면 resume 명령이
//! `claude -r <session_id> --settings "<경로>"` 가 된다(Claude Code 는 훅을 프로세스
//! 기동 시 한 번만 읽으므로, 살아있는 세션에 훅을 걸 유일한 창구는 이 재기동이다).
//! `--settings` 는 기존 훅을 대체가 아니라 **추가**하므로 tasty 내장 훅은 그대로
//! 살아 있다. 부착한 경로는 surface meta(`claude-session-profile`)에 남아 다음
//! 무인자 reboot 가 기본값으로 승계하고, `--clear-profile` 로 뗄 수 있다. 부착/승계된
//! 경로는 항상 재기동 전에 파일 존재 + JSON 파싱을 동기 검증한다(`resolve_and_apply_profile`)
//! — 검증 실패는 시퀀스를 아예 시작하지 않고 에러로 반환한다.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};

use crate::handlers::require_surface_id;

/// 명령 접수 → kill 시작까지 기본 대기 (초). `--delay` 로 오버라이드.
const DEFAULT_DELAY_SECS: u64 = 5;
/// Ctrl+C 전송 횟수 / 간격.
const CTRL_C_COUNT: u32 = 4;
const CTRL_C_INTERVAL: Duration = Duration::from_millis(500);
/// 전경 프로세스 폴링 간격.
const FG_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Ctrl+C 후 claude 종료(전경 이탈) 대기 한도.
const EXIT_WAIT: Duration = Duration::from_secs(5);
/// resume 명령 후 claude 복귀(전경 재진입) 대기 한도.
const RETURN_WAIT: Duration = Duration::from_secs(15);
/// 복귀 감지 후 TUI 입력 준비 grace. 프로세스는 전경에 즉시 잡히지만 TUI 가
/// 입력을 받기까지는 초기화(MCP 로딩 등)가 더 걸린다.
const TUI_READY_GRACE: Duration = Duration::from_secs(3);
/// 안내 프롬프트 제출 시도 횟수 / 재시도 간격 / 제출→화면 검증 대기.
/// TUI 초기화 중 입력은 소리 없이 유실되므로(실측), 제출 후 화면에 문구가
/// 나타났는지 `surface.screen_text` 로 확인하고 없으면 재시도한다.
const NOTICE_ATTEMPTS: u32 = 4;
const NOTICE_RETRY_INTERVAL: Duration = Duration::from_secs(3);
const NOTICE_VERIFY_DELAY: Duration = Duration::from_millis(1500);
/// 문구 확인 후 추가 Enter 전까지 대기. tell 의 본문/`\r` 분리 write 도 TUI 부팅
/// 직후엔 한 read burst 로 합쳐져 `\r` 이 paste 로 흡수될 수 있다(실측: 문구가
/// 입력창에 미제출로 잔류). 이미 제출된 경우 빈 입력창 Enter 는 no-op 이므로
/// 확인 후 별도 Enter 1회는 항상 안전하다.
const NOTICE_SUBMIT_DELAY: Duration = Duration::from_millis(500);
/// 화면 검증에 쓰는 문구 조각 — 안내문 선두라 112col 화면에서도 줄바꿈 없이
/// 붙어서 렌더된다.
const NOTICE_SNIPPET: &str = "tasty claude reboot";

/// surface meta 키 — 이 surface 에 부착된 Claude 세션 프로필(파일 경로). `--profile-file`
/// 로 부착하면 여기 기록되고, 인자 없는 다음 reboot 가 이 값을 기본으로 승계한다
/// (`claude-session-id`, session-start hook 이 기록하는 키와 나란히 reboot 가 관리).
const PROFILE_META_KEY: &str = "claude-session-profile";
/// surface meta 키 — 이 surface 에 부착된 Claude 세션 프로필의 **이름**(쉼표 구분,
/// `profile.rs` 레지스트리에 등록된 이름). `PROFILE_META_KEY`(경로)와 상호 배타적으로 관리한다 —
/// 이름으로 부착하면 여기 기록되고 `PROFILE_META_KEY`는 지운다. 다음 무인자
/// reboot 는 이 값이 있으면 이름을 **매번 다시 해석**한다(등록 내용이 그 사이
/// 갱신됐을 수 있으므로 경로를 그대로 캐시하지 않는다).
const PROFILE_NAMES_META_KEY: &str = "claude-session-profile-names";

/// `claude.reboot` 진입점. 검증·캡처를 동기로 끝내고 시퀀스는 background thread
/// 로 넘긴 뒤 즉시 응답한다 — 호출한 claude 가 턴을 마무리할 시간을 준다.
pub(crate) fn handle_reboot(
    inflight: &Arc<Mutex<HashSet<u32>>>,
    host: &HostHandle,
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface_id(params, tr)?;
    let (delay_secs, extra_prompt) = parse_reboot_options(params);
    let profile_action = parse_profile_option(params, tr)?;

    // 요청 시점 캡처 (session-end 가 meta 를 지우기 전).
    let session_id = fetch_session_id(host, surface_id, tr)?;
    if !is_safe_session_id(&session_id) {
        return Err(IpcMethodError::new(
            tr.t("claude.reboot.malformed_session_id")
                .replacen("{}", &surface_id.to_string(), 1)
                .replacen("{}", &format!("{session_id:?}"), 1),
        ));
    }

    // 부착/승계/해제 판정 + (있으면) 파일 존재+JSON 파싱 동기 검증. 실패 시 여기서
    // 즉시 반환 — 시퀀스(Ctrl+C 등)를 시작하지 않는다(깨진 프로필로 claude 기동이
    // 실패해 전경이 baseline 복귀 못 하고 방치되는 사고 방지).
    let profile_file = resolve_and_apply_profile(host, surface_id, &profile_action, data_dir, tr)?;

    let Some(baseline) = query_foreground(host, surface_id) else {
        return Err(IpcMethodError::new(tr.t_fmt(
            "claude.reboot.no_foreground_process",
            &surface_id.to_string(),
        )));
    };

    {
        let mut set = inflight.lock().map_err(|e| {
            IpcMethodError::new(tr.t_fmt("claude.reboot.lock_poisoned", &e.to_string()))
        })?;
        if !set.insert(surface_id) {
            return Err(IpcMethodError::new(tr.t_fmt(
                "claude.reboot.already_in_progress",
                &surface_id.to_string(),
            )));
        }
    }

    // 안내 프롬프트 본문은 spawn 전에 활성 locale 로 1 회 해석해 소유값으로
    // 넘긴다 — `Translator` 는 `self` 를 빌려 쓰므로 `'static` 백그라운드
    // 스레드로 이동할 수 없다.
    let notice_base = tr.t("claude.reboot.notice").to_string();

    let thread_host = host.clone();
    let thread_inflight = inflight.clone();
    let thread_session = session_id.clone();
    let spawned = thread::Builder::new()
        .name(format!("claude-reboot-s{surface_id}"))
        .spawn(move || {
            run_reboot_sequence(
                &thread_host,
                surface_id,
                delay_secs,
                &baseline,
                &thread_session,
                extra_prompt.as_deref(),
                profile_file.as_deref(),
                &notice_base,
            );
            if let Ok(mut set) = thread_inflight.lock() {
                set.remove(&surface_id);
            }
        });
    if let Err(e) = spawned {
        if let Ok(mut set) = inflight.lock() {
            set.remove(&surface_id);
        }
        return Err(IpcMethodError::new(
            tr.t_fmt("claude.reboot.spawn_thread_failed", &e.to_string()),
        ));
    }

    Ok(json!({
        "surface_id": surface_id,
        "session_id": session_id,
        "reboot_in_secs": delay_secs,
    }))
}

/// `--delay`(기본 5초) / `--prompt`(안내문 뒤에 덧붙일 추가 텍스트) 파싱.
pub(crate) fn parse_reboot_options(params: &Value) -> (u64, Option<String>) {
    let delay = params
        .get("delay")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_DELAY_SECS);
    let extra = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    (delay, extra)
}

/// `--profile-file` / `--profile` / `--clear-profile` 로 요청된 부착 상태 변경.
/// `Keep` 은 셋 다 없는 기본 호출 — 기존에 부착된 프로필(있으면)을 그대로 승계한다.
#[derive(Debug, PartialEq)]
pub(crate) enum ProfileOption {
    /// 인자 없음 — surface meta 에 부착된 값을 그대로 승계(없으면 프로필 없음).
    Keep,
    /// `--profile-file <path>` — 이 경로를 부착(meta 갱신). path 는 CLI
    /// `path_kind = "file"` 정규화를 이미 거친 절대경로다.
    AttachPath(String),
    /// `--profile <name[,name2,...]>` — 레지스트리 이름(들)을 부착. 둘 이상이면
    /// resolve 시점에 머지된다(`profile::resolve_names`).
    AttachNames(String),
    /// `--clear-profile` — 부착된 프로필을 뗀다(meta 삭제).
    Clear,
}

/// `--profile-file` / `--profile` / `--clear-profile` 파싱.
/// - `--clear-profile` 이 있으면 다른 인자와 무관하게 최우선(명시적 해제 의도가
///   새 부착보다 강하다).
/// - `--profile-file` 과 `--profile` 을 함께 주면 어느 쪽이 이기는지 조용히
///   정하지 않고 거부한다(last-wins 를 경로/이름 인자 사이에서도 반복하지 않기 위함).
pub(crate) fn parse_profile_option(
    params: &Value,
    tr: &Translator,
) -> Result<ProfileOption, IpcMethodError> {
    let clear = params
        .get("clear_profile")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if clear {
        return Ok(ProfileOption::Clear);
    }
    let path = params
        .get("profile_file")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let names = params
        .get("profile")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    match (path, names) {
        (Some(_), Some(_)) => Err(IpcMethodError::new(
            tr.t("claude.profile.mutually_exclusive_file_and_profile"),
        )),
        (Some(p), None) => Ok(ProfileOption::AttachPath(p.to_string())),
        (None, Some(n)) => Ok(ProfileOption::AttachNames(n.to_string())),
        (None, None) => Ok(ProfileOption::Keep),
    }
}

/// `ProfileOption` 을 이번 reboot 에 실제로 쓸 프로필 경로로 해석하고, 필요하면
/// surface meta 를 갱신한다. 해석된 경로가 있으면 파일 존재와 JSON 파싱을
/// 동기로 검증한다 — 새로 지정됐든 승계됐든(전에 유효했던 파일이 그 사이 지워지거나
/// 깨졌을 수 있다) 동일하게, 실패 시 reboot 시퀀스를 시작하지 않고 즉시 에러를
/// 반환한다. meta 쓰기는 검증을 통과한 뒤에만 하므로, 깨진 인자가 기존에 부착돼
/// 있던 정상 프로필을 덮어쓰지 않는다.
///
/// 이름 기반 부착(`AttachNames`)은 meta 에 **이름**을 저장하고, `Keep` 승계 시에도
/// 이름-meta 가 있으면 매번 다시 해석한다(등록 내용이 갱신됐을 수 있으므로 경로를
/// 캐시하지 않는다) — 경로-meta 는 이름-meta 가 없을 때만 폴백으로 본다.
fn resolve_and_apply_profile(
    host: &HostHandle,
    surface_id: u32,
    action: &ProfileOption,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Option<String>, IpcMethodError> {
    let candidate = match action {
        ProfileOption::Clear => None,
        ProfileOption::AttachPath(path) => Some(path.clone()),
        ProfileOption::AttachNames(names) => Some(resolve_names_to_path(data_dir, names, tr)?),
        ProfileOption::Keep => match fetch_profile_names_meta(host, surface_id) {
            Some(names) => Some(resolve_names_to_path(data_dir, &names, tr)?),
            None => fetch_profile_meta(host, surface_id),
        },
    };

    if let Some(path) = &candidate {
        validate_profile_file(path, tr)?;
    }

    match action {
        ProfileOption::AttachPath(path) => {
            set_profile_meta(host, surface_id, path);
            unset_profile_names_meta(host, surface_id);
        }
        ProfileOption::AttachNames(names) => {
            set_profile_names_meta(host, surface_id, names);
            unset_profile_meta(host, surface_id);
        }
        ProfileOption::Clear => {
            unset_profile_meta(host, surface_id);
            unset_profile_names_meta(host, surface_id);
        }
        ProfileOption::Keep => {}
    }

    Ok(candidate)
}

fn resolve_names_to_path(
    data_dir: Option<&Path>,
    names: &str,
    tr: &Translator,
) -> Result<String, IpcMethodError> {
    crate::profile::resolve_names(data_dir, names, tr)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| crate::profile::to_ipc_err(e, tr))
}

/// 프로필 파일이 존재하고 유효한 JSON 인지 동기 검증한다. 이 확인 없이 진행하면
/// claude 기동 자체가 실패해 전경이 baseline 으로 복귀하지 못하고, 안내 프롬프트도
/// 없이 시퀀스가 조용히 중단된다(사용자에게는 "reboot 했는데 claude 가 안 돌아왔다"
/// 로만 보인다) — 그 사고를 시작 전에 막는다.
fn validate_profile_file(path: &str, tr: &Translator) -> Result<(), IpcMethodError> {
    let contents = std::fs::read_to_string(path).map_err(|e| {
        IpcMethodError::new(
            tr.t("claude.reboot.profile_file_not_readable")
                .replacen("{}", path, 1)
                .replacen("{}", &e.to_string(), 1),
        )
    })?;
    serde_json::from_str::<Value>(&contents).map_err(|e| {
        IpcMethodError::new(
            tr.t("claude.reboot.profile_file_not_json")
                .replacen("{}", path, 1)
                .replacen("{}", &e.to_string(), 1),
        )
    })?;
    Ok(())
}

/// surface meta 에서 부착된 프로필 경로를 읽는다. 없으면 `None`(에러 아님 — 프로필
/// 없이 reboot 하는 것은 정상 상태).
fn fetch_profile_meta(host: &HostHandle, surface_id: u32) -> Option<String> {
    host.call(
        "surface.meta.get",
        json!({ "surface_id": surface_id, "key": PROFILE_META_KEY }),
    )
    .ok()
    .and_then(|r| r.get("value").and_then(|v| v.as_str()).map(String::from))
    .filter(|s| !s.is_empty())
}

fn set_profile_meta(host: &HostHandle, surface_id: u32, path: &str) {
    if let Err(e) = host.call(
        "surface.meta.set",
        json!({ "surface_id": surface_id, "key": PROFILE_META_KEY, "value": path }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: failed to record attached profile: {e}");
    }
}

fn unset_profile_meta(host: &HostHandle, surface_id: u32) {
    if let Err(e) = host.call(
        "surface.meta.unset",
        json!({ "surface_id": surface_id, "key": PROFILE_META_KEY }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: failed to clear attached profile: {e}");
    }
}

fn fetch_profile_names_meta(host: &HostHandle, surface_id: u32) -> Option<String> {
    host.call(
        "surface.meta.get",
        json!({ "surface_id": surface_id, "key": PROFILE_NAMES_META_KEY }),
    )
    .ok()
    .and_then(|r| r.get("value").and_then(|v| v.as_str()).map(String::from))
    .filter(|s| !s.is_empty())
}

fn set_profile_names_meta(host: &HostHandle, surface_id: u32, names: &str) {
    if let Err(e) = host.call(
        "surface.meta.set",
        json!({ "surface_id": surface_id, "key": PROFILE_NAMES_META_KEY, "value": names }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: failed to record attached profile names: {e}");
    }
}

fn unset_profile_names_meta(host: &HostHandle, surface_id: u32) {
    if let Err(e) = host.call(
        "surface.meta.unset",
        json!({ "surface_id": surface_id, "key": PROFILE_NAMES_META_KEY }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: failed to clear attached profile names: {e}");
    }
}

/// `claude.profile_current` 가 소비하는 "이 surface 에 지금 무엇이 부착돼 있나"
/// 조회. 이름-meta 를 우선하고(있으면 그게 실제 적용되는 것), 없으면 경로-meta.
/// 내장 훅은 별개 — `profile::list()` 가 항상 포함하므로 여기서는 세션별로
/// 달라지는 부착 상태만 다룬다.
pub(crate) struct AttachedProfile {
    pub names: Option<String>,
    pub path: Option<String>,
}

pub(crate) fn attached_profile_summary(host: &HostHandle, surface_id: u32) -> AttachedProfile {
    let names = fetch_profile_names_meta(host, surface_id);
    let path = if names.is_none() {
        fetch_profile_meta(host, surface_id)
    } else {
        None
    };
    AttachedProfile { names, path }
}

/// surface meta 에서 claude session id 를 읽는다. 없으면 에러 — hook 미설치이거나
/// 그 surface 에 살아있는 claude 세션이 없다는 뜻.
fn fetch_session_id(
    host: &HostHandle,
    surface_id: u32,
    tr: &Translator,
) -> Result<String, IpcMethodError> {
    let resp = host
        .call(
            "surface.meta.get",
            json!({ "surface_id": surface_id, "key": "claude-session-id" }),
        )
        .map_err(IpcMethodError::from)?;
    let session_id = resp
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if session_id.is_empty() {
        return Err(IpcMethodError::new(
            tr.t_fmt("claude.reboot.no_active_session", &surface_id.to_string()),
        ));
    }
    Ok(session_id)
}

/// session id 가 셸에 평문으로 들어가므로 uuid 계열 문자만 허용한다.
pub(crate) fn is_safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 셸에 전송할 resume 명령 (제출 `\r` 포함). 모든 셸(cmd/pwsh/bash)에서 동일하게
/// 동작하는 평문 — inline env prefix 는 붙이지 않는다(PTY env 에 `TASTY_SURFACE_ID`
/// 가 이미 주입돼 있고, `VAR=x cmd` 문법은 POSIX 전용이라 cmd.exe 에서 깨진다).
///
/// `profile_file` 이 있으면 `--settings "<path>"` 를 덧붙인다 — **인라인 JSON 이
/// 아니라 파일 경로**를 큰따옴표로 감싼다(중괄호/따옴표 이스케이프는 셸마다 달라
/// 인라인 JSON 은 같은 cmd/pwsh/bash 함정에 빠진다; 큰따옴표는 세 셸 모두 경로
/// 공백을 처리한다). 경로는 CLI `path_kind = "file"` 정규화를 이미 거쳐 절대경로다.
pub(crate) fn resume_command(session_id: &str, profile_file: Option<&str>) -> String {
    match profile_file {
        Some(path) => format!("claude -r {session_id} --settings \"{path}\"\r"),
        None => format!("claude -r {session_id}\r"),
    }
}

/// 안내 프롬프트 본문. `base` 는 활성 locale 로 이미 해석된 고정 문구
/// (`handle_reboot` 이 spawn 전에 1 회 계산). `--prompt` 추가 텍스트가 있으면
/// 빈 줄 뒤에 덧붙인다.
pub(crate) fn build_notice(base: &str, extra: Option<&str>) -> String {
    match extra {
        Some(t) => format!("{base}\n\n{t}"),
        None => base.to_string(),
    }
}

/// delay 후 전경 상태에 따른 다음 행동. 순수 함수 — 단위 테스트 대상.
#[derive(Debug, PartialEq)]
pub(crate) enum AfterDelay {
    /// 여전히 claude(baseline) — Ctrl+C 시퀀스로 종료시킨다.
    SendCtrlC,
    /// 이미 전경이 바뀜(스스로 종료 등) — 바로 resume 으로 간다.
    SkipToResume,
}

pub(crate) fn after_delay_action(current: &str, baseline: &str) -> AfterDelay {
    if current == baseline {
        AfterDelay::SendCtrlC
    } else {
        AfterDelay::SkipToResume
    }
}

/// 전체 시퀀스 (background thread). 각 단계 실패는 warn 로그 후 중단 —
/// 살아있는 TUI/셸에 잘못된 텍스트를 흘리지 않는 것이 최우선.
///
/// 인자가 많은 건 background thread 진입점이라 `&Translator` 를 넘길 수 없어
/// (`handle_reboot` 참고) 미리 해석해둔 `notice_base` 를 포함해 필요한 값을
/// 전부 소유값으로 펼쳐 받기 때문 — 묶을 만한 자연스러운 하위 구조가 없다.
#[allow(clippy::too_many_arguments)]
fn run_reboot_sequence(
    host: &HostHandle,
    surface_id: u32,
    delay_secs: u64,
    baseline: &str,
    session_id: &str,
    extra_prompt: Option<&str>,
    profile_file: Option<&str>,
    notice_base: &str,
) {
    thread::sleep(Duration::from_secs(delay_secs));

    let Some(current) = query_foreground(host, surface_id) else {
        tracing::warn!("claude reboot s{surface_id}: surface gone before kill — aborting");
        return;
    };

    if !kill_or_skip(host, surface_id, baseline, &current) {
        return;
    }

    if !resume_and_wait(host, surface_id, baseline, session_id, profile_file) {
        return;
    }
    thread::sleep(TUI_READY_GRACE);

    if !deliver_notice(
        host,
        surface_id,
        baseline,
        &build_notice(notice_base, extra_prompt),
    ) {
        tracing::warn!(
            "claude reboot s{surface_id}: notice not confirmed on screen after {NOTICE_ATTEMPTS} attempts"
        );
    }
}

/// delay 후 판정(`AfterDelay`)에 따라 Ctrl+C 로 종료시키거나(SendCtrlC) 이미 바뀐
/// 전경을 그대로 인정하고 넘어간다(SkipToResume). kill 실패 시 `false`.
fn kill_or_skip(host: &HostHandle, surface_id: u32, baseline: &str, current: &str) -> bool {
    match after_delay_action(current, baseline) {
        AfterDelay::SendCtrlC => kill_claude_via_ctrlc(host, surface_id, baseline),
        AfterDelay::SkipToResume => {
            tracing::info!(
                "claude reboot s{surface_id}: foreground already '{current}' (was '{baseline}') — skipping Ctrl+C"
            );
            true
        }
    }
}

/// Ctrl+C ×N 전송 후 전경이 baseline 에서 이탈할 때까지 확인. 실패 시 `false` —
/// 살아있는 claude TUI 입력창에 resume 명령이 타이핑되는 사고 방지.
fn kill_claude_via_ctrlc(host: &HostHandle, surface_id: u32, baseline: &str) -> bool {
    for _ in 0..CTRL_C_COUNT {
        if let Err(e) = host.call(
            "surface.send_combo",
            json!({ "surface_id": surface_id, "key": "c", "modifiers": ["ctrl"] }),
        ) {
            tracing::warn!("claude reboot s{surface_id}: send_combo failed: {e} — aborting");
            return false;
        }
        thread::sleep(CTRL_C_INTERVAL);
    }
    // 종료 확인: 전경이 baseline 에서 이탈할 때까지. 실패 시 절대 진행 금지 —
    // 살아있는 claude TUI 입력창에 resume 명령이 타이핑되는 사고 방지.
    if !poll_foreground(host, surface_id, EXIT_WAIT, |name| name != baseline) {
        tracing::warn!(
            "claude reboot s{surface_id}: claude still in foreground after {CTRL_C_COUNT}x Ctrl+C — aborting (nothing sent)"
        );
        return false;
    }
    true
}

/// resume 명령 전송 + 전경이 baseline(claude 계열 이름)으로 돌아올 때까지 확인.
/// 미복귀면 `false` — 안내 프롬프트도 보내지 않는다(셸 프롬프트에 평문이 명령으로
/// 실행되는 사고 방지).
fn resume_and_wait(
    host: &HostHandle,
    surface_id: u32,
    baseline: &str,
    session_id: &str,
    profile_file: Option<&str>,
) -> bool {
    if let Err(e) = host.call(
        "surface.send",
        json!({ "surface_id": surface_id, "text": resume_command(session_id, profile_file) }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: resume send failed: {e}");
        return false;
    }

    if !poll_foreground(host, surface_id, RETURN_WAIT, |name| name == baseline) {
        tracing::warn!(
            "claude reboot s{surface_id}: claude did not return to foreground within {}s — resume sent but notice skipped",
            RETURN_WAIT.as_secs()
        );
        return false;
    }
    true
}

/// 안내 프롬프트를 제출하고 화면에 실제로 나타났는지 검증한다. TUI 초기화 중
/// PTY 입력이 유실될 수 있어(실측: 복귀 직후 tell 이 소리 없이 사라짐) 확인될
/// 때까지 재시도한다. 매 시도 전 전경이 여전히 claude(baseline)인지 재확인 —
/// resume 이 실패해 셸로 떨어진 경우 평문이 셸 명령으로 실행되는 사고 방지.
fn deliver_notice(host: &HostHandle, surface_id: u32, baseline: &str, notice: &str) -> bool {
    for attempt in 1..=NOTICE_ATTEMPTS {
        match try_deliver_notice_once(host, surface_id, baseline, notice) {
            NoticeAttempt::Confirmed => return true,
            NoticeAttempt::Aborted => return false,
            NoticeAttempt::NotYetVisible => {
                tracing::info!(
                    "claude reboot s{surface_id}: notice attempt {attempt}/{NOTICE_ATTEMPTS} not visible yet — retrying"
                );
                thread::sleep(NOTICE_RETRY_INTERVAL);
            }
        }
    }
    // 마지막 시도 직후 verify 가 아슬하게 놓쳤을 수 있으니 한 번 더 확인.
    if screen_contains(host, surface_id, NOTICE_SNIPPET) {
        ensure_submitted(host, surface_id);
        return true;
    }
    false
}

/// 안내 프롬프트 제출 1회 시도 결과.
enum NoticeAttempt {
    /// 화면에서 확인, 제출까지 완료.
    Confirmed,
    /// 전경 변경/tell 실패 — 시퀀스 전체를 중단해야 함.
    Aborted,
    /// 제출은 했으나 아직 화면에 안 보임 — 재시도 대상.
    NotYetVisible,
}

/// 전경이 여전히 baseline(claude) 인지 확인 후 안내 프롬프트를 `terminal.tell` 로
/// 제출하고 화면에 나타났는지 검사한다.
fn try_deliver_notice_once(
    host: &HostHandle,
    surface_id: u32,
    baseline: &str,
    notice: &str,
) -> NoticeAttempt {
    match query_foreground(host, surface_id) {
        Some(name) if name == baseline => {}
        other => {
            tracing::warn!(
                "claude reboot s{surface_id}: foreground changed to {other:?} before notice — aborting"
            );
            return NoticeAttempt::Aborted;
        }
    }
    if let Err(e) = host.call(
        "terminal.tell",
        json!({ "surface": surface_id, "text": notice }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: notice tell failed: {e}");
        return NoticeAttempt::Aborted;
    }
    thread::sleep(NOTICE_VERIFY_DELAY);
    if screen_contains(host, surface_id, NOTICE_SNIPPET) {
        ensure_submitted(host, surface_id);
        NoticeAttempt::Confirmed
    } else {
        NoticeAttempt::NotYetVisible
    }
}

/// 문구가 화면에 있어도 제출(`\r`)이 paste 로 흡수돼 입력창에 잔류할 수 있으므로
/// 별도 Enter 를 한 번 더 보낸다. 이미 제출된 상태면 빈 입력창 Enter 라 no-op.
fn ensure_submitted(host: &HostHandle, surface_id: u32) {
    thread::sleep(NOTICE_SUBMIT_DELAY);
    if let Err(e) = host.call(
        "surface.send_key",
        json!({ "surface_id": surface_id, "key": "enter" }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: extra submit enter failed: {e}");
    }
}

/// `surface.screen_text` 로 현재 화면에 문구가 보이는지 확인. 실패 → false.
fn screen_contains(host: &HostHandle, surface_id: u32, needle: &str) -> bool {
    host.call("surface.screen_text", json!({ "surface_id": surface_id }))
        .ok()
        .and_then(|r| {
            r.get("text")
                .and_then(|t| t.as_str())
                .map(|t| t.contains(needle))
        })
        .unwrap_or(false)
}

/// `surface.foreground_process` 1회 조회. 실패/이름 없음 → None.
fn query_foreground(host: &HostHandle, surface_id: u32) -> Option<String> {
    let resp = host
        .call(
            "surface.foreground_process",
            json!({ "surface_id": surface_id }),
        )
        .ok()?;
    resp.get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// 전경 이름이 조건을 만족할 때까지 폴링. 조회 실패(surface 소멸)는 즉시 false.
fn poll_foreground(
    host: &HostHandle,
    surface_id: u32,
    timeout: Duration,
    pred: impl Fn(&str) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match query_foreground(host, surface_id) {
            Some(name) if pred(&name) => return true,
            Some(_) => {}
            None => return false,
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(FG_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_translator() -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, "en")
    }

    #[test]
    fn parse_defaults_delay_5_and_no_prompt() {
        let (delay, extra) = parse_reboot_options(&json!({ "surface_id": 1 }));
        assert_eq!(delay, 5);
        assert_eq!(extra, None);
    }

    #[test]
    fn parse_explicit_delay_and_prompt() {
        let (delay, extra) =
            parse_reboot_options(&json!({ "delay": 2, "prompt": "빌드부터 다시 확인" }));
        assert_eq!(delay, 2);
        assert_eq!(extra.as_deref(), Some("빌드부터 다시 확인"));
    }

    #[test]
    fn parse_empty_prompt_treated_as_none() {
        let (_, extra) = parse_reboot_options(&json!({ "prompt": "" }));
        assert_eq!(extra, None);
    }

    #[test]
    fn safe_session_id_accepts_uuid() {
        assert!(is_safe_session_id("0e5cbdf4-32a1-4a5c-9c1d-8f2b3a4c5d6e"));
        assert!(is_safe_session_id("abc_DEF-123"));
    }

    #[test]
    fn safe_session_id_rejects_shell_metachars() {
        assert!(!is_safe_session_id(""));
        assert!(!is_safe_session_id("abc; rm -rf /"));
        assert!(!is_safe_session_id("a b"));
        assert!(!is_safe_session_id("a$(x)"));
        assert!(!is_safe_session_id("a&b"));
    }

    #[test]
    fn resume_command_is_plain_and_submits() {
        assert_eq!(
            resume_command("0e5cbdf4-32a1", None),
            "claude -r 0e5cbdf4-32a1\r"
        );
    }

    #[test]
    fn resume_command_with_profile_appends_quoted_settings_path() {
        assert_eq!(
            resume_command("0e5cbdf4-32a1", Some("/home/user/profile.json")),
            "claude -r 0e5cbdf4-32a1 --settings \"/home/user/profile.json\"\r"
        );
    }

    #[test]
    fn parse_profile_option_defaults_to_keep() {
        assert_eq!(
            parse_profile_option(&json!({ "surface_id": 1 }), &test_translator()).unwrap(),
            ProfileOption::Keep
        );
    }

    #[test]
    fn parse_profile_option_attach_path() {
        assert_eq!(
            parse_profile_option(&json!({ "profile_file": "/a/b.json" }), &test_translator())
                .unwrap(),
            ProfileOption::AttachPath("/a/b.json".to_string())
        );
    }

    #[test]
    fn parse_profile_option_attach_names() {
        assert_eq!(
            parse_profile_option(
                &json!({ "profile": "reviewer,sandbox" }),
                &test_translator()
            )
            .unwrap(),
            ProfileOption::AttachNames("reviewer,sandbox".to_string())
        );
    }

    #[test]
    fn parse_profile_option_path_and_names_together_is_rejected() {
        assert!(
            parse_profile_option(
                &json!({ "profile_file": "/a/b.json", "profile": "reviewer" }),
                &test_translator()
            )
            .is_err()
        );
    }

    #[test]
    fn parse_profile_option_clear_wins_over_attach() {
        assert_eq!(
            parse_profile_option(
                &json!({ "profile_file": "/a/b.json", "clear_profile": true }),
                &test_translator()
            )
            .unwrap(),
            ProfileOption::Clear
        );
    }

    #[test]
    fn parse_profile_option_clear_wins_over_names() {
        assert_eq!(
            parse_profile_option(
                &json!({ "profile": "reviewer", "clear_profile": true }),
                &test_translator()
            )
            .unwrap(),
            ProfileOption::Clear
        );
    }

    #[test]
    fn parse_profile_option_empty_profile_file_treated_as_keep() {
        assert_eq!(
            parse_profile_option(&json!({ "profile_file": "" }), &test_translator()).unwrap(),
            ProfileOption::Keep
        );
    }

    #[test]
    fn validate_profile_file_accepts_valid_json() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), r#"{"hooks":{}}"#).unwrap();
        assert!(validate_profile_file(tmp.path().to_str().unwrap(), &test_translator()).is_ok());
    }

    #[test]
    fn validate_profile_file_rejects_missing_file() {
        assert!(
            validate_profile_file("/no/such/tasty-profile-test.json", &test_translator()).is_err()
        );
    }

    #[test]
    fn validate_profile_file_rejects_broken_json() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(tmp.path(), "{not valid json").unwrap();
        assert!(validate_profile_file(tmp.path().to_str().unwrap(), &test_translator()).is_err());
    }

    #[test]
    fn notice_without_extra_is_fixed_text() {
        let base = test_translator().t("claude.reboot.notice").to_string();
        assert_eq!(build_notice(&base, None), base);
    }

    #[test]
    fn notice_with_extra_appends_after_blank_line() {
        let base = test_translator().t("claude.reboot.notice").to_string();
        let n = build_notice(&base, Some("이어서 soak 돌려"));
        assert!(n.starts_with(&base));
        assert!(n.ends_with("\n\n이어서 soak 돌려"));
    }

    #[test]
    fn after_delay_same_foreground_sends_ctrl_c() {
        assert_eq!(after_delay_action("node", "node"), AfterDelay::SendCtrlC);
    }

    #[test]
    fn after_delay_changed_foreground_skips_to_resume() {
        assert_eq!(
            after_delay_action("cmd.exe", "node"),
            AfterDelay::SkipToResume
        );
    }
}
