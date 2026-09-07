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
//!
//! 부착 상태는 surface meta 와 **함께** session id 로 키잉한 부착 기록
//! ([`crate::profile_attach`])에도 남는다 — surface meta 는 앱 재시작/닫은 탭 복원을
//! 넘지 못하므로, 복원된 프로세스에 프로필을 다시 붙이려면 meta 바깥의 사본이
//! 필요하다. 두 저장처가 갈라지지 않도록 갱신은 [`record_update_for`] 한 곳에서
//! meta 4 분기와 1:1 로 대응시킨다.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_plugin_agent_common::reboot::{ensure_submitted, is_safe_session_id, parse_options};
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};

use crate::handlers::require_target_surface;

/// in-flight 집합의 락. 임계구역이 `HashSet<u32>` 의 insert/remove 뿐이라 패닉이 지나가도
/// 남는 값이 성립한다 — 복구가 답이다.
///
/// **획득과 해제의 답이 다르다.** 획득(`insert`)은 실패를 호출자에게 에러로 돌려주면
/// 그만이고 전용 문구(`claude.reboot.lock_poisoned`)까지 있다. 반면 해제(`remove`)를
/// 조용히 건너뛰면 그 `surface_id` 가 집합에 **영구히 남아** 이후 모든 reboot 이
/// `already_in_progress` 로 거절된다. poison 은 sticky 라 한 번 걸리면 모든 surface 의
/// 해제가 같이 막혀 기능 전체가 잠기고, 되돌릴 경로가 plugin 재시작뿐이다.
const INFLIGHT_WHAT: &str = "the claude reboot in-flight set";
static INFLIGHT_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
/// 화면 검증에 쓰는 문구 조각 — 안내문 선두라 112col 화면에서도 줄바꿈 없이
/// 붙어서 렌더된다.
const NOTICE_SNIPPET: &str = "tasty claude reboot";

/// surface meta 키 — 이 surface 에 부착된 Claude 세션 프로필(파일 경로). `--profile-file`
/// 로 부착하면 여기 기록되고, 인자 없는 다음 reboot 가 이 값을 기본으로 승계한다
/// (`claude-session-id`, session-start hook 이 기록하는 키와 나란히 reboot 가 관리).
pub(crate) const PROFILE_META_KEY: &str = "claude-session-profile";
/// surface meta 키 — 이 surface 에 부착된 Claude 세션 프로필의 **이름**(쉼표 구분,
/// `profile.rs` 레지스트리에 등록된 이름). `PROFILE_META_KEY`(경로)와 상호 배타적으로 관리한다 —
/// 이름으로 부착하면 여기 기록되고 `PROFILE_META_KEY`는 지운다. 다음 무인자
/// reboot 는 이 값이 있으면 이름을 **매번 다시 해석**한다(등록 내용이 그 사이
/// 갱신됐을 수 있으므로 경로를 그대로 캐시하지 않는다).
pub(crate) const PROFILE_NAMES_META_KEY: &str = "claude-session-profile-names";

/// `claude.reboot` 진입점. 검증·캡처를 동기로 끝내고 시퀀스는 background thread
/// 로 넘긴 뒤 즉시 응답한다 — 호출한 claude 가 턴을 마무리할 시간을 준다.
pub(crate) fn handle_reboot(
    inflight: &Arc<Mutex<HashSet<u32>>>,
    host: &HostHandle,
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_target_surface(params, tr)?;
    reboot_surface(inflight, host, surface_id, params, data_dir, tr)
}

/// 대상 surface 가 이미 정해진 상태의 reboot 본체. `handle_reboot` 은 `--surface`
/// 를 읽어서, `child-profile`(`handlers::handle_child_profile`)은 `--child` 를
/// surface id 로 해석해서 각각 여기로 들어온다 — 대상 해석만 다르고 그 뒤의
/// 검증·캡처·시퀀스는 **한 벌만** 존재한다(부착 경로를 복제하지 않는다).
///
/// `inflight` 도 그대로 공유하므로 두 명령이 같은 surface 를 동시에 태우면
/// 뒤엣것이 `already_in_progress` 로 거부된다.
pub(crate) fn reboot_surface(
    inflight: &Arc<Mutex<HashSet<u32>>>,
    host: &HostHandle,
    surface_id: u32,
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let (delay_secs, extra_prompt) = parse_options(params);
    // host 왕복 없이 판정 가능한 프로필 검증(상호배타 · 이름 해석 · 파일 파싱)은
    // 전부 여기서 끝낸다 — 뒤따르는 어떤 부수효과(meta 갱신 · Ctrl+C 시퀀스)보다
    // 앞이라, 잘못된 인자로는 대상이 죽지 않는다.
    let (profile_action, preresolved) = preflight_profile(params, data_dir, tr)?;

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
    let profile_file = resolve_and_apply_profile(
        host,
        surface_id,
        &session_id,
        &profile_action,
        preresolved,
        data_dir,
        tr,
    )?;

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
            tasty_utils::poison::recover_mutex(
                thread_inflight.lock(),
                INFLIGHT_WHAT,
                &INFLIGHT_POISON_REPORTED,
            )
            .remove(&surface_id);
        });
    if let Err(e) = spawned {
        tasty_utils::poison::recover_mutex(
            inflight.lock(),
            INFLIGHT_WHAT,
            &INFLIGHT_POISON_REPORTED,
        )
        .remove(&surface_id);
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

/// 프로필 인자를 **host 를 건드리지 않고** 끝까지 검증한다 — 상호배타 판정
/// (`parse_profile_option`), 등록 이름 해석, 그리고 결과 파일의 존재+JSON 파싱.
///
/// [`reboot_surface`] 가 세션 조회보다도 먼저 이걸 부르는 이유는 실패 지점을
/// 앞으로 당기기 위해서다: 여기서 거부되면 surface meta 도 안 바뀌고 Ctrl+C 도
/// 안 나가므로, 오타난 프로필 이름 하나로 멀쩡한 (자식) 세션이 죽는 일이 없다.
///
/// `Keep`(무인자 승계)만 surface meta 를 읽어야 해서 여기서 후보를 못 정한다 —
/// `None` 을 돌려주고 [`resolve_and_apply_profile`] 이 이어서 해석한다.
pub(crate) fn preflight_profile(
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<(ProfileOption, Option<String>), IpcMethodError> {
    let action = parse_profile_option(params, tr)?;
    let candidate = match &action {
        ProfileOption::AttachPath(path) => Some(path.clone()),
        ProfileOption::AttachNames(names) => Some(resolve_names_to_path(data_dir, names, tr)?),
        ProfileOption::Clear | ProfileOption::Keep => None,
    };
    if let Some(path) = &candidate {
        validate_profile_file(path, tr)?;
    }
    Ok((action, candidate))
}

/// `ProfileOption` 을 이번 reboot 에 실제로 쓸 프로필 경로로 해석하고, 필요하면
/// surface meta 를 갱신한다. 해석된 경로가 있으면 파일 존재와 JSON 파싱을
/// 동기로 검증한다 — 새로 지정됐든 승계됐든(전에 유효했던 파일이 그 사이 지워지거나
/// 깨졌을 수 있다) 동일하게, 실패 시 reboot 시퀀스를 시작하지 않고 즉시 에러를
/// 반환한다. meta 쓰기는 검증을 통과한 뒤에만 하므로, 깨진 인자가 기존에 부착돼
/// 있던 정상 프로필을 덮어쓰지 않는다.
///
/// 명시 부착(`AttachPath`/`AttachNames`)의 해석·검증은 [`preflight_profile`] 이
/// host 접촉 전에 이미 끝내 `preresolved` 로 넘겨준다 — 여기서 새로 해석·검증하는
/// 것은 `Keep`(무인자 승계) 뿐이다. 어느 쪽이든 위 보증은 그대로다.
///
/// 이름 기반 부착(`AttachNames`)은 meta 에 **이름**을 저장하고, `Keep` 승계 시에도
/// 이름-meta 가 있으면 매번 다시 해석한다(등록 내용이 갱신됐을 수 있으므로 경로를
/// 캐시하지 않는다) — 경로-meta 는 이름-meta 가 없을 때만 폴백으로 본다.
fn resolve_and_apply_profile(
    host: &HostHandle,
    surface_id: u32,
    session_id: &str,
    action: &ProfileOption,
    preresolved: Option<String>,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Option<String>, IpcMethodError> {
    let candidate = match action {
        ProfileOption::Clear => None,
        ProfileOption::AttachPath(_) | ProfileOption::AttachNames(_) => preresolved,
        ProfileOption::Keep => {
            let candidate = match fetch_profile_names_meta(host, surface_id) {
                Some(names) => Some(resolve_names_to_path(data_dir, &names, tr)?),
                None => fetch_profile_meta(host, surface_id),
            };
            if let Some(path) = &candidate {
                validate_profile_file(path, tr)?;
            }
            candidate
        }
    };

    // meta 갱신과 **같은 지점**에서 session id 키 부착 기록도 갱신한다 — 한쪽만
    // 갱신되는 경로가 생기면 복원 시 meta 와 기록이 다른 프로필을 가리킨다.
    // 기록은 meta 가 못 넘는 복원 경계를 넘기 위한 사본이다(`profile_attach` 모듈 doc).
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
    apply_record_update(data_dir, session_id, &record_update_for(action));

    Ok(candidate)
}

/// 부착 기록에 가할 변경. meta 갱신 4 분기와 1:1 로 대응한다 — 판정을 순수 함수로
/// 떼어내 두 저장처(meta / 기록)가 갈라지지 않는지 단위 테스트로 고정한다.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RecordUpdate {
    Store(crate::profile_attach::AttachRecord),
    Remove,
    /// 승계(`Keep`)는 부착 상태를 바꾸지 않으므로 기록도 그대로 둔다.
    Keep,
}

pub(crate) fn record_update_for(action: &ProfileOption) -> RecordUpdate {
    use crate::profile_attach::AttachRecord;
    match action {
        // 이름 부착은 **이름**을 남긴다 — 복원 시점에 다시 해석해야 최신 등록
        // 내용이 반영된다(경로를 캐시하면 meta 쪽 계약과 어긋난다).
        ProfileOption::AttachNames(names) => {
            RecordUpdate::Store(AttachRecord::Names(names.clone()))
        }
        ProfileOption::AttachPath(path) => RecordUpdate::Store(AttachRecord::Path(path.clone())),
        ProfileOption::Clear => RecordUpdate::Remove,
        ProfileOption::Keep => RecordUpdate::Keep,
    }
}

fn apply_record_update(data_dir: Option<&Path>, session_id: &str, update: &RecordUpdate) {
    match update {
        RecordUpdate::Store(record) => crate::profile_attach::store(data_dir, session_id, record),
        RecordUpdate::Remove => crate::profile_attach::remove(data_dir, session_id),
        RecordUpdate::Keep => {}
    }
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
pub(crate) fn validate_profile_file(path: &str, tr: &Translator) -> Result<(), IpcMethodError> {
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

/// 셸에 전송할 resume 명령 (제출 `\r` 포함). 모든 셸(cmd/pwsh/bash)에서 동일하게
/// 동작하는 평문 — inline env prefix 는 붙이지 않는다(PTY env 에 `TASTY_SURFACE_ID`
/// 가 이미 주입돼 있고, `VAR=x cmd` 문법은 POSIX 전용이라 cmd.exe 에서 깨진다).
///
/// `profile_file` 이 있으면 `--settings "<path>"` 를 덧붙인다 — **인라인 JSON 이
/// 아니라 파일 경로**를 큰따옴표로 감싼다(중괄호/따옴표 이스케이프는 셸마다 달라
/// 인라인 JSON 은 같은 cmd/pwsh/bash 함정에 빠진다; 큰따옴표는 세 셸 모두 경로
/// 공백을 처리한다). 경로는 CLI `path_kind = "file"` 정규화를 이미 거쳐 절대경로다.
pub(crate) fn resume_command(session_id: &str, profile_file: Option<&str>) -> String {
    format!("{}\r", resume_command_line(session_id, profile_file))
}

/// resume 명령의 **본문**(제출 `\r` 없음). `restore.command` surface meta 도 같은
/// 문자열을 써야 하므로(복원은 이 meta 를 셸에 그대로 타이핑한다) 포맷의 단일
/// 소유자를 여기 둔다 — 셸 전송용은 [`resume_command`] 가 `\r` 만 덧붙인다.
pub(crate) fn resume_command_line(session_id: &str, profile_file: Option<&str>) -> String {
    match profile_file {
        Some(path) => format!("claude -r {session_id} --settings \"{path}\""),
        None => format!("claude -r {session_id}"),
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
        ensure_submitted(host, surface_id, "claude");
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
        ensure_submitted(host, surface_id, "claude");
        NoticeAttempt::Confirmed
    } else {
        NoticeAttempt::NotYetVisible
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

    // ── 프로필 인자 preflight (todo/52 확인 절차 3·4번) ──
    //
    // `preflight_profile` 은 `host` 를 **인자로 받지 않는다** — 시그니처 자체가
    // "여기서 거부되면 Ctrl+C 도 meta 갱신도 일어날 수 없다" 는 보증이다.
    // `reboot_surface` 가 세션 조회보다도 먼저 이 함수를 부른다.

    #[test]
    fn preflight_rejects_profile_and_profile_file_together() {
        let tr = test_translator();
        let dir = tempfile::tempdir().unwrap();
        let err = preflight_profile(
            &json!({ "profile": "probe", "profile_file": "/tmp/x.json" }),
            Some(dir.path()),
            &tr,
        )
        .unwrap_err();
        assert!(
            err.message.contains("mutually exclusive"),
            "{}",
            err.message
        );
    }

    #[test]
    fn preflight_rejects_an_unregistered_profile_name() {
        let tr = test_translator();
        let dir = tempfile::tempdir().unwrap();
        let err =
            preflight_profile(&json!({ "profile": "nosuch" }), Some(dir.path()), &tr).unwrap_err();
        assert!(err.message.contains("nosuch"), "{}", err.message);
    }

    #[test]
    fn preflight_resolves_a_registered_profile_name_to_a_real_file() {
        let tr = test_translator();
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.json");
        std::fs::write(&src, r#"{"hooks":{}}"#).unwrap();
        crate::profile::register(Some(dir.path()), "probe", &src).unwrap();

        let (action, resolved) =
            preflight_profile(&json!({ "profile": "probe" }), Some(dir.path()), &tr).unwrap();
        assert!(matches!(action, ProfileOption::AttachNames(ref n) if n == "probe"));
        let resolved = resolved.expect("registered name resolves to a merged file");
        assert!(Path::new(&resolved).is_file(), "{resolved}");
    }

    #[test]
    fn preflight_leaves_keep_unresolved_for_the_meta_lookup() {
        // 무인자 승계는 surface meta 를 읽어야 하므로 여기서 후보를 못 정한다.
        let tr = test_translator();
        let dir = tempfile::tempdir().unwrap();
        let (action, resolved) = preflight_profile(&json!({}), Some(dir.path()), &tr).unwrap();
        assert!(matches!(action, ProfileOption::Keep));
        assert_eq!(resolved, None);
    }

    /// R1 — 부착 4 분기가 meta 와 **같은 값**을 기록에 남기는지. 이름 부착은
    /// 해석된 경로가 아니라 이름 문자열이 남아야 한다(복원 시 재해석 대상).
    #[test]
    fn attach_names_records_the_names_not_a_path() {
        assert_eq!(
            record_update_for(&ProfileOption::AttachNames("reviewer".into())),
            RecordUpdate::Store(crate::profile_attach::AttachRecord::Names(
                "reviewer".into()
            ))
        );
    }

    #[test]
    fn attach_path_records_the_path() {
        assert_eq!(
            record_update_for(&ProfileOption::AttachPath("/abs/p.json".into())),
            RecordUpdate::Store(crate::profile_attach::AttachRecord::Path(
                "/abs/p.json".into()
            ))
        );
    }

    /// R1 — `--clear-profile` 은 meta 두 키와 기록을 **함께** 지운다. meta 쪽
    /// (`unset_profile_meta`/`unset_profile_names_meta`)은 host 호출이라 여기서
    /// 직접 검증할 수 없어 실행 시나리오 검증이 담당하고, 이 테스트는 기록 쪽이
    /// 같은 분기에서 빠지지 않는지를 고정한다.
    #[test]
    fn clear_removes_the_record() {
        assert_eq!(
            record_update_for(&ProfileOption::Clear),
            RecordUpdate::Remove
        );
    }

    #[test]
    fn keep_leaves_the_record_untouched() {
        assert_eq!(record_update_for(&ProfileOption::Keep), RecordUpdate::Keep);
    }

    /// 기록 갱신이 실제 파일에 반영되는지 — 부착 → 조회 왕복.
    #[test]
    fn applying_an_attach_update_round_trips_through_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        apply_record_update(
            Some(tmp.path()),
            "sess-1",
            &record_update_for(&ProfileOption::AttachNames("reviewer".into())),
        );
        assert_eq!(
            crate::profile_attach::load(Some(tmp.path()), "sess-1"),
            Some(crate::profile_attach::AttachRecord::Names(
                "reviewer".into()
            ))
        );
        apply_record_update(
            Some(tmp.path()),
            "sess-1",
            &record_update_for(&ProfileOption::Clear),
        );
        assert_eq!(
            crate::profile_attach::load(Some(tmp.path()), "sess-1"),
            None
        );
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
