//! `claude.hook` IPC 핸들러.
//!
//! CLI에서 `tasty claude hook <event> [--surface <id>] [--session <s>]`로 호출되며,
//! event별로 자식 상태(idle/needs_input/active)와 session meta, 텔레메트리를 산출한다.
//!
//! **idle/needs_input 신호는 자체 state 대신 호스트 registry(`terminal.set_state`)로
//! 주입한다**(occupancy-05). caller(spawn/tell 호출 주체)에게 완료를 알리는 건
//! `handlers.rs`의 1회성 알림 훅(`register_notify_hooks`/`claude.notify_done`)이
//! 이 모듈이 fire 하는 `claude-idle`/`needs-input` 이벤트를 구독해 처리한다 — 이
//! 모듈은 parent 관계를 조회하거나 fan-out 하지 않는다. wall-time 텔레메트리
//! 타이밍만 plugin 이 보유한다(`ClaudeState`).
//!
//! state 변이/host side effect 계산을 [`apply_hook`]으로 분리해 단위 테스트에서는
//! host 호출을 모킹하지 않고도 분기 로직을 검증할 수 있게 했다.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};

use crate::checklist;
use crate::error_scan::ErrorScanner;
use crate::profile_attach::{self, AttachRecord};
use crate::state::ClaudeState;

/// surface meta 키 — 복원이 셸에 그대로 타이핑하는 재기동 명령
/// (`src/core/layout_persistence/restore.rs`). 값 포맷의 소유자는
/// [`crate::reboot::resume_command_line`] 다.
pub(crate) const RESTORE_COMMAND_META_KEY: &str = "restore.command";

/// hook 처리 후 plugin이 호스트에 보낼 IPC 호출 1건.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCall {
    /// `terminal.set_state { surface, state }` — 호스트 registry 의 자식 상태 갱신.
    /// state ∈ {"idle", "needs_input", "active"}.
    SetState {
        surface_id: u32,
        state: &'static str,
    },
    /// `surface.fire_hook { surface_id, event }`
    FireHook {
        surface_id: u32,
        event: &'static str,
    },
    /// `surface.meta.set { surface_id, key, value }`
    MetaSet {
        surface_id: u32,
        key: &'static str,
        value: String,
    },
    /// `surface.meta.unset { surface_id, key }`
    MetaUnset { surface_id: u32, key: &'static str },
    /// `telemetry.record { metric, value, tags }`
    TelemetryRecord {
        metric: &'static str,
        value: f64,
        surface_id: u32,
    },
    /// `surface.completion { surface_id, kind }` — surface highlight(주의 환기) 발동.
    /// "Claude가 명령을 끝냈다"/"확인이 필요하다" 신호를
    /// `docs/features/surface-highlight/index.md` 의 producer 중립 highlight API 로
    /// 연결한다. `kind` ∈ {"completion", "needs_input"} — 호스트의
    /// `AttentionKind` 를 그대로 미러링한다(`terminal.set_state` 의 `state` 문자열과
    /// 동형 패턴). stop/subagent-stop/session-end 는 `"completion"`,
    /// notification(비-idle_prompt)/pre-tool-use(AskUserQuestion) 는
    /// `"needs_input"`.
    SurfaceCompletion { surface_id: u32, kind: &'static str },
}

pub fn handle_claude_hook(
    state: &mut ClaudeState,
    scanner: &Arc<Mutex<ErrorScanner>>,
    host: &HostHandle,
    params: &Value,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let event = params
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.hook.missing_event")))?;

    let surface_id = resolve_surface_id(params, tr)?;
    let session = params
        .get("session")
        .and_then(|v| v.as_str())
        .map(String::from);
    let message = params.get("message").and_then(|v| v.as_str());
    let notification_type = params.get("notification_type").and_then(|v| v.as_str());
    let now_ms = now_ms();

    let mut calls = apply_hook(event, surface_id, session.as_deref(), notification_type, tr)?;

    // 부착된 프로필을 복원 명령에 싣는다 — surface meta 는 앱 재시작/닫은 탭 복원을
    // 넘지 못하므로(`profile_attach` 모듈 doc), 복원된 프로세스에 프로필을 다시
    // 붙이는 유일한 창구가 이 `restore.command` 문자열이다. `apply_hook` 은
    // host/data_dir 없는 순수 함수라 여기(둘 다 가진 호출부)에서 처리한다.
    if event == "session-start"
        && let Some(session_id) = session.as_deref()
    {
        let meta = crate::reboot::attached_profile_summary(host, surface_id);
        let plan = plan_session_start_profile(session_id, &meta, data_dir, tr);
        apply_session_start_profile(&mut calls, surface_id, session_id, &plan);
        // 프로필이 확정됐으면 기록을 항상 다시 찍는다(re-stamp). reboot 시퀀스의
        // Ctrl+C 가 발화시키는 session-end 가 방금 쓴 기록을 지우므로, 이 재기록이
        // 없으면 **모든 reboot 이 기록을 영구 소실**시킨다(reboot → session-end →
        // session-start 순서).
        if let Some(record) = &plan.restamp {
            profile_attach::store(data_dir, session_id, record);
        }
        // 훅이 발화하지 못한 경로(탭 close, 강제 종료)가 남긴 orphan 회수.
        profile_attach::sweep(data_dir);
    }

    calls.extend(telemetry_for_hook(
        state, event, surface_id, message, now_ms,
    ));

    for call in &calls {
        deliver(host, call);
    }

    if is_new_turn_event(event) {
        reset_dedupe_if_enabled(scanner, surface_id);
    }

    // `continue-checklist` 는 이 hook 이벤트 흐름과 별개 경로(`claude.checklist_hook`)로
    // 발동하지만, 라운드 상태는 Claude Code 의 session_id 로 키잉되므로 세션 종료를
    // 아는 이 지점(전역 `session-end`)에서 함께 정리해야 orphan 파일이 남지 않는다.
    if event == "session-end" {
        checklist::remove_state_for_session(data_dir, session.as_deref().unwrap_or(""));
        // 프로필 부착 기록은 여기서 **종료 표시**만 한다 — 즉시 삭제하지 않는 이유는
        // `profile_attach` 모듈 doc "수명" 절 참고(탭을 닫으면 이 훅이 정상 발화하는데,
        // 곧바로 이어지는 Ctrl+Shift+T 복원이 기록을 다시 필요로 한다). reboot 중
        // 발화하는 session-end 도 여기로 들어오지만, 뒤이은 session-start 의 re-stamp 가
        // 표시를 지운다.
        profile_attach::mark_ended(data_dir, session.as_deref().unwrap_or(""));
    }

    Ok(json!({ "ok": true, "surface_id": surface_id, "event": event }))
}

/// 새 턴 시작(=idle 상태 해제) 신호 — `apply_hook` 이 `SetState{state:"active"}` 로
/// 묶는 이벤트 집합과 동일하다. 이 시점에 error dedupe 를 초기화해, 지난 턴의 에러
/// 텍스트로 눌려있던 dedupe 가 이번 턴에 나는 새 에러까지 억제하지 않게 한다.
fn is_new_turn_event(event: &str) -> bool {
    matches!(event, "prompt-submit" | "session-start" | "active")
}

/// scanner 가 이 surface 를 추적 중일 때만 dedupe 를 초기화한다 — error_scan 은
/// launch/spawn/respawn 이 등록한 surface 만 enable 하므로, 그 밖의(예: 이미
/// `terminal.release` 로 관계가 끊겨 disable 된) surface 의 hook 이벤트는 조용히
/// 건너뛴다.
fn reset_dedupe_if_enabled(scanner: &Arc<Mutex<ErrorScanner>>, surface_id: u32) {
    let mut s = crate::error_scan::lock_scanner(scanner);
    if s.is_enabled(surface_id) {
        s.reset_dedupe(surface_id);
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// hook event → telemetry HostCall 매핑. 순수 함수 — 테스트가 직접 검증한다.
///
/// - `session-start` → state 에 시작 시각 기록 (HostCall 없음)
/// - `stop` / `subagent-stop` / `session-end` → wall_time_ms 가 있으면 발행
/// - `notification` → message 에서 `\btokens?:\s*(\d+)\b` 매칭되면 input_tokens 발행
pub fn telemetry_for_hook(
    state: &mut ClaudeState,
    event: &str,
    surface_id: u32,
    message: Option<&str>,
    now_ms: u64,
) -> Vec<HostCall> {
    let mut out = Vec::new();
    match event {
        "session-start" => {
            state.mark_session_start(surface_id, now_ms);
        }
        "stop" | "subagent-stop" | "session-end" => {
            if let Some(elapsed) = state.take_wall_time(surface_id, now_ms) {
                out.push(HostCall::TelemetryRecord {
                    metric: "wall_time_ms",
                    value: elapsed as f64,
                    surface_id,
                });
            }
        }
        "notification" => {
            if let Some(text) = message
                && let Some(n) = extract_tokens(text)
            {
                out.push(HostCall::TelemetryRecord {
                    metric: "input_tokens",
                    value: n as f64,
                    surface_id,
                });
            }
        }
        _ => {}
    }
    out
}

/// `\btokens?:\s*(\d+)\b` 휴리스틱. 정규식 dep 추가를 피하려고 수동 스캔.
fn extract_tokens(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // word boundary: i==0 또는 이전 char 가 alnum 아니면 통과
        let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if prev_ok && (bytes[i..].starts_with(b"token") || bytes[i..].starts_with(b"Token")) {
            let mut j = i + 5;
            if j < bytes.len() && (bytes[j] == b's' || bytes[j] == b'S') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b':' {
                j += 1;
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                let start = j;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > start {
                    let after_ok = j == bytes.len() || !bytes[j].is_ascii_alphanumeric();
                    if after_ok
                        && let Ok(n) = std::str::from_utf8(&bytes[start..j])
                            .unwrap()
                            .parse::<u64>()
                    {
                        return Some(n);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// event → 자식 상태 갱신(`SetState`) + session meta 계산. host 에 의존하지 않는
/// 순수 함수 — 단위 테스트가 분기 동작을 직접 검증한다. 자식 상태
/// (idle/needs_input/active)는 호스트 registry 로 주입할 `SetState` HostCall 로
/// 표현한다(자체 state 미보유).
///
/// `notification_type` 은 `"notification"` 이벤트에서만 쓰인다 — Claude Code 의
/// Notification stdin payload 에 실려오는 `notification_type` 필드(값 예:
/// `permission_prompt`/`idle_prompt`/`auth_success`/`elicitation_*`, 실측으로
/// 필드 존재와 `permission_prompt` 값을 확인함). `idle_prompt`(무입력 대기 —
/// 사용자가 자리를 비웠을 뿐 실제로 응답을 기다리는 질문이 없는 상태)만 배제하고
/// 그 외(알려지지 않은 값·필드 없음 포함)는 기존과 동일하게 needs_input 을 켠다 —
/// 알려진 유일한 오탐 케이스만 걷어내고 나머지는 안전하게 넓게 유지하는 deny-list.
pub fn apply_hook(
    event: &str,
    surface_id: u32,
    session: Option<&str>,
    notification_type: Option<&str>,
    tr: &Translator,
) -> Result<Vec<HostCall>, IpcMethodError> {
    let mut calls = Vec::new();
    match event {
        "stop" | "subagent-stop" => {
            calls.push(HostCall::SetState {
                surface_id,
                state: "idle",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
            calls.push(HostCall::SurfaceCompletion {
                surface_id,
                kind: "completion",
            });
        }
        "session-end" => {
            // 이 발화가 정상적인 세션 종료인지, 아니면 예상 밖 타이밍(중복
            // Ctrl+C, 크래시 등)에 잘못 발화해 `claude-session-id` meta 를
            // 지워버린 것인지는 이 함수만으로 구분할 수 없다 — 사고(2026-08-05,
            // surface 3095) 재발 시 이 로그와 session-start 로그의 순서를 대조해
            // 판단한다.
            tracing::info!(
                "claude hook session-end s{surface_id}: clearing claude-session-id/restore.command meta"
            );
            calls.push(HostCall::SetState {
                surface_id,
                state: "idle",
            });
            calls.push(HostCall::MetaUnset {
                surface_id,
                key: "claude-session-id",
            });
            calls.push(HostCall::MetaUnset {
                surface_id,
                key: RESTORE_COMMAND_META_KEY,
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "claude-idle",
            });
            calls.push(HostCall::SurfaceCompletion {
                surface_id,
                kind: "completion",
            });
        }
        "notification" => {
            // idle_prompt(무입력 대기)는 실제로 대기 중인 질문이 없는 오탐 케이스라
            // needs_input 을 켜지 않는다 — 그 외(permission_prompt 등 알려진 값,
            // 알려지지 않은 값, 필드 자체가 없는 경우)는 기존과 동일하게 켠다.
            if notification_type != Some("idle_prompt") {
                calls.push(HostCall::SetState {
                    surface_id,
                    state: "needs_input",
                });
                calls.push(HostCall::FireHook {
                    surface_id,
                    event: "needs-input",
                });
                calls.push(HostCall::SurfaceCompletion {
                    surface_id,
                    kind: "needs_input",
                });
            }
        }
        "pre-tool-use" => {
            // matcher `"AskUserQuestion"` 로 이미 좁혀 등록되므로(install.rs
            // `MANAGED_HOOKS`) 이 event 가 오면 곧 선택지 UI 가 뜬다는 뜻이다 —
            // Notification("permission_prompt")과 동일한 효과를 구조적으로 더
            // 정밀하게(tool 이름 자체가 보증) 낸다.
            calls.push(HostCall::SetState {
                surface_id,
                state: "needs_input",
            });
            calls.push(HostCall::FireHook {
                surface_id,
                event: "needs-input",
            });
            calls.push(HostCall::SurfaceCompletion {
                surface_id,
                kind: "needs_input",
            });
        }
        "post-tool-use" => {
            // matcher `"AskUserQuestion"` 로 좁혀 등록 — 사용자가 답변을 제출한
            // 직후 발화한다(실측: `duration_ms: 0`). `UserPromptSubmit` 은 이 답변에
            // 대해 발화하지 않으므로(질문/답변이 같은 prompt turn 안의 tool
            // 상호작용이라 새 프롬프트로 집계되지 않음, 실측 확인) 이 이벤트가
            // needs_input 해제의 유일한 신호다.
            calls.push(HostCall::SetState {
                surface_id,
                state: "active",
            });
        }
        "prompt-submit" | "session-start" | "active" => {
            calls.push(HostCall::SetState {
                surface_id,
                state: "active",
            });
            if event == "session-start" {
                match session {
                    Some(session_id) => {
                        calls.push(HostCall::MetaSet {
                            surface_id,
                            key: "claude-session-id",
                            value: session_id.to_string(),
                        });
                        calls.push(HostCall::MetaSet {
                            surface_id,
                            key: RESTORE_COMMAND_META_KEY,
                            // 프로필이 붙어 있으면 호출부(`handle_claude_hook`)가 이
                            // 값을 `--settings` 포함 형태로 다시 쓴다 — 여기서는
                            // host/data_dir 없이 결정 가능한 기본형만 만든다.
                            value: crate::reboot::resume_command_line(session_id, None),
                        });
                    }
                    None => {
                        // stdin JSON 에 session_id 가 없었다 — `read_stdin_json`
                        // 이 None 을 반환했거나(TTY/파싱 실패), Claude Code 가
                        // payload 에 session_id 를 채우지 않은 케이스. 이 경로를
                        // 타면 `claude-session-id` meta 가 기록되지 않아 이후
                        // `tasty claude reboot` 가 "meta not set" 으로 실패한다
                        // (사고 2026-08-05, surface 3095).
                        tracing::warn!(
                            "claude hook session-start s{surface_id}: no session_id in payload — claude-session-id meta NOT set (tasty claude reboot will fail until the next session-start carries a session_id)"
                        );
                    }
                }
            }
        }
        other => {
            return Err(IpcMethodError::invalid_params(
                &tr.t_fmt("claude.hook.unknown_event", other),
            ));
        }
    }
    Ok(calls)
}

/// session-start 가 복원할 프로필 계획.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct SessionStartProfile {
    /// 기록에서 되살린 경우 meta 를 되돌리기 위한 호출(meta 가 이미 있었으면 빈 vec).
    pub meta_calls: Vec<HostCall>,
    /// 이번 세션에 실제로 실을 프로필 파일 경로. `None` 이면 프로필 없이 진행한다.
    pub profile_file: Option<String>,
    /// 다시 찍을 부착 기록 — 프로필이 확정됐을 때만 `Some`.
    pub restamp: Option<AttachRecord>,
}

/// "이 세션에 무엇이 붙어 있나" 를 판정한다.
///
/// 우선순위는 **surface meta → 부착 기록** 이다. meta 가 있으면 그건 살아있는
/// 세션의 정상 발화(예: `reboot` 의 resume)라 복구할 것이 없고, meta 가 비어 있을
/// 때만 복원(새 surface id 발급 + meta purge)을 건넌 것으로 보고 기록에서 되살린다.
///
/// 이름은 **매번 다시 해석**한다 — 등록 내용이 그 사이 갱신됐을 수 있고(`profile-register`),
/// 같은 이름이 프로필에서 게이트로(혹은 그 반대로) 재등록됐을 수도 있으므로 해석
/// 결과 경로를 캐시하지 않는다(`profile::resolve_names` 의 계약).
///
/// **해석 실패는 에러가 아니라 조용한 강등이다** — `reboot` 은 같은 상황에서 에러를
/// 반환하고 시퀀스를 시작조차 하지 않지만(깨진 프로필로 claude 가 기동 실패해 전경이
/// 방치되는 사고 방지), 이 경로에는 에러를 돌려줄 상대가 없다. 여기서 실패시키면
/// `restore.command` 가 아예 기록되지 않아 **복원 자체가 깨진다**. 프로필만 빠뜨리고
/// 세션 복원은 살리는 쪽이 손실이 작으므로 warn 만 남기고 진행한다.
pub(crate) fn plan_session_start_profile(
    session_id: &str,
    meta: &crate::reboot::AttachedProfile,
    data_dir: Option<&Path>,
    tr: &Translator,
) -> SessionStartProfile {
    let (attached, from_record) = match (&meta.names, &meta.path) {
        (Some(names), _) => (Some(AttachRecord::Names(names.clone())), false),
        (None, Some(path)) => (Some(AttachRecord::Path(path.clone())), false),
        (None, None) => (profile_attach::load(data_dir, session_id), true),
    };
    let Some(record) = attached else {
        return SessionStartProfile::default();
    };

    let resolved = match &record {
        AttachRecord::Names(names) => {
            match crate::profile::resolve_names(data_dir, names, tr) {
                Ok(path) => Some(path.to_string_lossy().into_owned()),
                Err(e) => {
                    // 부착 후 unregister 됐거나 파일이 깨진 경우.
                    tracing::warn!(
                        "claude hook session-start {session_id}: profile names {names:?} no longer resolve ({e:?}) — restoring without a profile"
                    );
                    None
                }
            }
        }
        AttachRecord::Path(path) => match crate::reboot::validate_profile_file(path, tr) {
            Ok(()) => Some(path.clone()),
            Err(e) => {
                tracing::warn!(
                    "claude hook session-start {session_id}: attached profile file {path:?} is unusable ({}) — restoring without a profile",
                    e.message
                );
                None
            }
        },
    };

    let Some(profile_file) = resolved else {
        // 강등: meta 도 기록도 건드리지 않는다 — 사용자가 이름을 다시 등록하면
        // 다음 session-start 가 그대로 복구한다.
        return SessionStartProfile::default();
    };

    let meta_calls = if from_record {
        vec![match &record {
            AttachRecord::Names(names) => HostCall::MetaSet {
                surface_id: 0,
                key: crate::reboot::PROFILE_NAMES_META_KEY,
                value: names.clone(),
            },
            AttachRecord::Path(path) => HostCall::MetaSet {
                surface_id: 0,
                key: crate::reboot::PROFILE_META_KEY,
                value: path.clone(),
            },
        }]
    } else {
        Vec::new()
    };

    SessionStartProfile {
        meta_calls,
        profile_file: Some(profile_file),
        restamp: Some(record),
    }
}

/// 계획을 `apply_hook` 이 만든 호출 목록에 반영한다 — `restore.command` 값을
/// `--settings` 포함 형태로 다시 쓰고, 기록에서 되살린 meta 를 덧붙인다. 순수 함수.
pub(crate) fn apply_session_start_profile(
    calls: &mut Vec<HostCall>,
    surface_id: u32,
    session_id: &str,
    plan: &SessionStartProfile,
) {
    if let Some(path) = &plan.profile_file {
        for call in calls.iter_mut() {
            if let HostCall::MetaSet { key, value, .. } = call
                && *key == RESTORE_COMMAND_META_KEY
            {
                *value = crate::reboot::resume_command_line(session_id, Some(path));
            }
        }
    }
    calls.extend(plan.meta_calls.iter().map(|call| match call {
        // 계획 단계는 surface 를 모른다(순수하게 "무엇을" 만 정한다) — 여기서 채운다.
        HostCall::MetaSet { key, value, .. } => HostCall::MetaSet {
            surface_id,
            key,
            value: value.clone(),
        },
        other => other.clone(),
    }));
}

/// host 호출을 쏘고 **실패해도 계속 간다** — 이 함수는 절대 전파하지 않는다.
///
/// 전파하면 안 되는 이유는 호출부의 순서다. `handle_claude_hook` 은 이 루프 *뒤에*
/// 로컬 정리를 한다(session-end 의 `checklist::remove_state_for_session` ·
/// `profile_attach::mark_ended`, session-start 의 `store`/`sweep`). 그 넷은
/// 파일시스템만 만지므로 surface 가 없어도 정의되는 일인데, 여기서 `?` 로 끊으면
/// **surface 가 사라진 바로 그때** 건너뛴다 — 정리가 필요한 유일한 경우에만 정리가
/// 안 도는 셈이다.
///
/// 게다가 그 경우는 예외가 아니라 상례다: 탭을 닫으면 호스트가 레이아웃에서 surface
/// 를 먼저 지우고 그 다음 PTY 를 떨구므로, PTY 사망이 발화시키는 `session-end` 훅은
/// **언제나** 이미 없는 surface 를 가리킨다.
///
/// codex 의 `handle_hook` 이 `terminal.set_state` 실패를 전파하는 것은 표류가 아니라
/// 같은 규칙의 반대편이다 — 거기엔 호출 뒤에 지킬 로컬 상태가 없다. 규칙 전문은
/// [error-handling](../../../docs/dev-guide/error-handling.md)
/// "plugin 핸들러의 host 호출 — 전파와 최선노력".
fn deliver(host: &HostHandle, call: &HostCall) {
    let (method, params) = match call {
        HostCall::SetState { surface_id, state } => (
            "terminal.set_state",
            json!({ "surface": surface_id, "state": state }),
        ),
        HostCall::FireHook { surface_id, event } => (
            "surface.fire_hook",
            json!({ "surface_id": surface_id, "event": event }),
        ),
        HostCall::MetaSet {
            surface_id,
            key,
            value,
        } => (
            "surface.meta.set",
            json!({ "surface_id": surface_id, "key": key, "value": value }),
        ),
        HostCall::MetaUnset { surface_id, key } => (
            "surface.meta.unset",
            json!({ "surface_id": surface_id, "key": key }),
        ),
        HostCall::TelemetryRecord {
            metric,
            value,
            surface_id,
        } => (
            "telemetry.record",
            json!({
                "metric": metric,
                "value": value,
                "tags": { "surface_id": surface_id.to_string() },
            }),
        ),
        HostCall::SurfaceCompletion { surface_id, kind } => (
            "surface.completion",
            json!({ "surface_id": surface_id, "kind": kind }),
        ),
    };
    if let Err(e) = host.call(method, params) {
        tracing::warn!("claude hook host call '{method}' failed: {e}");
    }
}

/// `surface` 해석의 판정부 — 환경을 읽지 않아 결정적이다.
///
/// 환경변수를 직접 읽으면 테스트가 그것을 피하려고 자기를 건너뛰게 되고(그 변수는 우리
/// 실행 환경에 **항상** 있다), 그러면 초록은 "위반 없음" 이 아니라 "한 번도 안 돎" 을
/// 뜻하게 된다. 그래서 env 값을 인자로 받는다.
fn resolve_surface_id_from(
    params: &Value,
    env_surface: Option<&str>,
    tr: &Translator,
) -> Result<u32, IpcMethodError> {
    // **값이 왔는데 숫자가 아닌 것**과 **키가 아예 없는 것**을 가른다. 둘을 합치면
    // 잘못된 값이 조용히 env 폴백으로 넘어가고, 그 폴백은 호출자 **자신**이라
    // 명령이 자기에게 배달된다 — 종료코드 0, 오류 없음.
    //
    // `u32` 범위 밖도 같은 부류다. `as u32` 로 자르면 `5_000_000_000` 이 `705_032_704`
    // 가 되는데, 그것은 **실재할 수 있는 다른 surface 의 id** 다(실측). 못 읽는 값이
    // 자기에게 가는 것보다 나쁘다 — 남의 터미널로 간다. 자르지 말고 거부한다.
    for key in ["surface", "surface_id"] {
        let Some(raw) = params.get(key) else { continue };
        if raw.is_null() {
            continue;
        }
        return match raw.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(sid) => Ok(sid),
            None => Err(IpcMethodError::invalid_params(
                &tr.t_fmt("claude.params.surface_not_a_number", &raw.to_string()),
            )),
        };
    }
    if let Some(sid) = env_surface.and_then(|e| e.parse::<u32>().ok()) {
        return Ok(sid);
    }
    Err(IpcMethodError::invalid_params(
        tr.t("claude.params.missing_surface_no_env"),
    ))
}

/// 위 판정부에 실제 환경을 물려주는 얇은 껍데기.
fn resolve_surface_id(params: &Value, tr: &Translator) -> Result<u32, IpcMethodError> {
    let env = std::env::var("TASTY_SURFACE_ID").ok();
    resolve_surface_id_from(params, env.as_deref(), tr)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 crate `lang/` 을 로드한 `Translator` — `checklist.rs` SENTINEL 핀
    /// 테스트와 동일 패턴(lang 파일 드리프트로부터 assertion 을 고정).
    fn test_translator() -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, "en")
    }

    #[test]
    fn stop_sets_idle_and_emits_fire_hook() {
        let calls = apply_hook("stop", 100, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "idle",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "claude-idle",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "completion",
                },
            ]
        );
    }

    #[test]
    fn subagent_stop_treated_like_stop() {
        let calls = apply_hook("subagent-stop", 7, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 7,
                    state: "idle",
                },
                HostCall::FireHook {
                    surface_id: 7,
                    event: "claude-idle",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 7,
                    kind: "completion",
                },
            ]
        );
    }

    #[test]
    fn notification_sets_needs_input_and_fires_needs_input() {
        let calls = apply_hook("notification", 100, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "needs_input",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "needs-input",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "needs_input",
                },
            ]
        );
    }

    #[test]
    fn notification_permission_prompt_sets_needs_input() {
        let calls = apply_hook(
            "notification",
            100,
            None,
            Some("permission_prompt"),
            &test_translator(),
        )
        .unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "needs_input",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "needs-input",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "needs_input",
                },
            ]
        );
    }

    #[test]
    fn notification_idle_prompt_does_not_set_needs_input() {
        // 실측 확인: idle_prompt 는 "사용자가 자리를 비웠다" 는 뜻이지 실제로 대기
        // 중인 질문이 없다 — needs_input 오탐의 근원이므로 배제한다.
        let calls = apply_hook(
            "notification",
            100,
            None,
            Some("idle_prompt"),
            &test_translator(),
        )
        .unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn notification_unknown_type_still_sets_needs_input() {
        // 알려지지 않은 notification_type(또는 향후 신설 값)은 안전하게 기존
        // 동작(needs_input 켬)을 유지한다 — 오탐이 확실한 idle_prompt 만 배제.
        let calls = apply_hook(
            "notification",
            100,
            None,
            Some("auth_success"),
            &test_translator(),
        )
        .unwrap();
        assert!(!calls.is_empty());
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SetState {
                state: "needs_input",
                ..
            }
        )));
    }

    #[test]
    fn notification_missing_type_still_sets_needs_input() {
        // stdin 에 notification_type 자체가 없는 (구버전 Claude Code 등) 경우도
        // 기존 동작을 유지한다 — 회귀 방지.
        let calls = apply_hook("notification", 100, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SetState {
                state: "needs_input",
                ..
            }
        )));
    }

    #[test]
    fn pre_tool_use_sets_needs_input_and_fires_needs_input() {
        // matcher "AskUserQuestion" 로 이미 좁혀 등록되므로 이 event 가 오면 곧
        // 선택지 UI 가 뜬다는 뜻 — Notification 의 needs_input 분기와 동일한 효과.
        let calls = apply_hook("pre-tool-use", 100, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "needs_input",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "needs-input",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "needs_input",
                },
            ]
        );
    }

    #[test]
    fn post_tool_use_sets_active() {
        // 실측 확인: AskUserQuestion 답변은 UserPromptSubmit 을 발생시키지 않으므로
        // PostToolUse 가 needs_input 해제의 유일한 신호. highlight 는 다시 안
        // 올린다(이미 질문에 답한 것이지 "완료/확인 필요" 신호가 아님).
        let calls = apply_hook("post-tool-use", 100, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![HostCall::SetState {
                surface_id: 100,
                state: "active",
            }]
        );
    }

    #[test]
    fn session_end_clears_session_meta_and_fires_idle() {
        let calls = apply_hook("session-end", 100, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "idle",
                },
                HostCall::MetaUnset {
                    surface_id: 100,
                    key: "claude-session-id",
                },
                HostCall::MetaUnset {
                    surface_id: 100,
                    key: "restore.command",
                },
                HostCall::FireHook {
                    surface_id: 100,
                    event: "claude-idle",
                },
                HostCall::SurfaceCompletion {
                    surface_id: 100,
                    kind: "completion",
                },
            ]
        );
    }

    // ── surface highlight 배선(`docs/features/surface-highlight/index.md`) —
    // stop/subagent-stop/session-end/notification 각각에서 SurfaceCompletion 이
    // 나가는지 개별 확인 — 위 4개 테스트가 이미 전체 시퀀스에 포함됨을 검증하지만,
    // 여기서는 "완료/확인필요 신호에는 항상 highlight 가 딸려온다"는 계약 자체를
    // 명시적으로 이름 붙여 pin 한다 ──

    #[test]
    fn stop_also_raises_surface_completion_highlight() {
        let calls = apply_hook("stop", 100, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 100,
                kind: "completion",
            }
        )));
    }

    #[test]
    fn subagent_stop_also_raises_surface_completion_highlight() {
        let calls = apply_hook("subagent-stop", 7, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 7,
                kind: "completion",
            }
        )));
    }

    #[test]
    fn session_end_also_raises_surface_completion_highlight() {
        let calls = apply_hook("session-end", 100, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 100,
                kind: "completion",
            }
        )));
    }

    #[test]
    fn notification_also_raises_surface_completion_highlight() {
        let calls = apply_hook("notification", 100, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 100,
                kind: "needs_input",
            }
        )));
    }

    /// pre-tool-use(AskUserQuestion) 도 needs_input kind 로 highlight 를 발동한다 —
    /// notification 과 동일 계약.
    #[test]
    fn pre_tool_use_also_raises_surface_completion_with_needs_input_kind() {
        let calls = apply_hook("pre-tool-use", 100, None, None, &test_translator()).unwrap();
        assert!(calls.iter().any(|c| matches!(
            c,
            HostCall::SurfaceCompletion {
                surface_id: 100,
                kind: "needs_input",
            }
        )));
    }

    #[test]
    fn prompt_submit_does_not_raise_surface_completion_highlight() {
        // "작업 시작" 신호는 완료/확인필요가 아니므로 highlight 대상이 아니다 —
        // apply_hook의 "prompt-submit"|"session-start"|"active" 분기는 건드리지 않는다
        // (`docs/features/surface-highlight/index.md` 참고).
        let calls = apply_hook("prompt-submit", 100, None, None, &test_translator()).unwrap();
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, HostCall::SurfaceCompletion { .. }))
        );
    }

    #[test]
    fn prompt_submit_sets_active_and_no_meta() {
        let calls = apply_hook("prompt-submit", 100, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![HostCall::SetState {
                surface_id: 100,
                state: "active",
            }]
        );
    }

    #[test]
    fn session_start_without_session_id_just_sets_active() {
        let calls = apply_hook("session-start", 100, None, None, &test_translator()).unwrap();
        assert_eq!(
            calls,
            vec![HostCall::SetState {
                surface_id: 100,
                state: "active",
            }]
        );
    }

    #[test]
    fn session_start_with_session_id_emits_meta_set() {
        let calls = apply_hook(
            "session-start",
            100,
            Some("sess-abc"),
            None,
            &test_translator(),
        )
        .unwrap();
        assert_eq!(
            calls,
            vec![
                HostCall::SetState {
                    surface_id: 100,
                    state: "active",
                },
                HostCall::MetaSet {
                    surface_id: 100,
                    key: "claude-session-id",
                    value: "sess-abc".into(),
                },
                HostCall::MetaSet {
                    surface_id: 100,
                    key: "restore.command",
                    value: "claude -r sess-abc".into(),
                },
            ]
        );
    }

    // ── 프로필 복원 (session-start) ──────────────────────────────────────

    fn register_profile(dir: &std::path::Path, name: &str, body: &str) {
        let src = dir.join(format!("{name}-src.json"));
        std::fs::write(&src, body).unwrap();
        crate::profile::register(Some(dir), name, &src).unwrap();
    }

    fn register_gate(dir: &std::path::Path, name: &str) {
        let body = dir.join(format!("{name}-body.md"));
        std::fs::write(&body, format!("{name} 본문\n[[{name}-DONE]]\n")).unwrap();
        crate::gate::register(
            Some(dir),
            name,
            &body,
            Some(&format!("[[{name}-DONE]]")),
            Some(2),
        )
        .unwrap();
    }

    fn no_meta() -> crate::reboot::AttachedProfile {
        crate::reboot::AttachedProfile {
            names: None,
            path: None,
        }
    }

    fn names_meta(names: &str) -> crate::reboot::AttachedProfile {
        crate::reboot::AttachedProfile {
            names: Some(names.to_string()),
            path: None,
        }
    }

    /// session-start 호출 목록에서 `restore.command` 값을 뽑는다.
    fn restore_command(calls: &[HostCall]) -> Option<String> {
        calls.iter().find_map(|c| match c {
            HostCall::MetaSet { key, value, .. } if *key == RESTORE_COMMAND_META_KEY => {
                Some(value.clone())
            }
            _ => None,
        })
    }

    /// session-start 전체 경로(순수 부분)를 한 번에 돌린다 — `apply_hook` 산출물에
    /// 계획을 반영한 최종 호출 목록.
    fn session_start_calls(
        surface_id: u32,
        session_id: &str,
        meta: &crate::reboot::AttachedProfile,
        data_dir: &std::path::Path,
    ) -> (Vec<HostCall>, SessionStartProfile) {
        let tr = test_translator();
        let mut calls =
            apply_hook("session-start", surface_id, Some(session_id), None, &tr).unwrap();
        let plan = plan_session_start_profile(session_id, meta, Some(data_dir), &tr);
        apply_session_start_profile(&mut calls, surface_id, session_id, &plan);
        (calls, plan)
    }

    /// R2 (3) — meta 는 비었고 기록만 있을 때, 기록으로 프로필을 복구해
    /// `restore.command` 에 `--settings` 를 싣는다.
    #[test]
    fn session_start_restores_the_profile_from_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "reviewer", r#"{"env":{"A":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("reviewer".into()),
        );

        let (calls, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());

        let generated = plan.profile_file.expect("프로필이 해석돼야 한다");
        // 프로덕션은 `data_dir.join("profiles").join("generated")` 로 만들므로 Windows
        // 에서는 `\` 로 구분된다 — 문자열 비교가 아니라 `Path::ends_with` 로 컴포넌트를
        // 견준다(std 의 `Path` 는 Windows 에서 `/` 도 구분자로 인정하므로 패턴은 그대로
        // 쓸 수 있다). 실패 메시지에는 기대값과 실제값을 함께 남긴다 — 경로만 찍으면
        // CI 로그만 보고는 무엇이 어긋났는지 알 수 없다.
        let expected = "profiles/generated/reviewer.json";
        assert!(
            std::path::Path::new(&generated).ends_with(expected),
            "생성 프로필 경로가 `{expected}` 로 끝나야 하는데 실제로는 `{generated}` 다"
        );
        assert_eq!(
            restore_command(&calls).unwrap(),
            format!("claude -r sess-1 --settings \"{generated}\"")
        );
        // meta 도 함께 되살아난다 — 이후 `profile-current` / 무인자 reboot 가 승계한다.
        assert!(calls.contains(&HostCall::MetaSet {
            surface_id: 7,
            key: crate::reboot::PROFILE_NAMES_META_KEY,
            value: "reviewer".into(),
        }));
    }

    /// R2 (4) — meta 가 이미 있으면(살아있는 세션의 정상 발화) 기록을 보지 않는다.
    /// 기록에 meta 와 다른, **해석 불가능한** 이름을 넣어 두고도 meta 값으로
    /// 해석되면 기록을 조회하지 않았다는 뜻이다.
    #[test]
    fn session_start_prefers_meta_over_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "reviewer", r#"{"env":{"A":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("ghost".into()),
        );

        let (calls, plan) = session_start_calls(7, "sess-1", &names_meta("reviewer"), tmp.path());

        assert!(plan.profile_file.unwrap().ends_with("reviewer.json"));
        assert!(restore_command(&calls).unwrap().contains("--settings"));
        // meta 가 이미 있으므로 meta 복구 호출은 없다.
        assert!(plan.meta_calls.is_empty());
        // re-stamp 는 meta 값으로 기록을 덮어써 두 저장처를 다시 맞춘다.
        assert_eq!(plan.restamp, Some(AttachRecord::Names("reviewer".into())));
    }

    /// R3 (5) — 부착된 이름이 그 사이 unregister 되면 에러가 아니라 조용한 강등.
    /// 복원 자체는 살아야 하므로 `restore.command` 는 프로필 없는 형태로 남는다.
    #[test]
    fn session_start_degrades_when_the_recorded_name_no_longer_resolves() {
        let tmp = tempfile::tempdir().unwrap();
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("ghost".into()),
        );

        let (calls, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());

        assert_eq!(plan, SessionStartProfile::default());
        assert_eq!(restore_command(&calls).unwrap(), "claude -r sess-1");
    }

    /// R3 — 경로 부착도 같은 강등 규칙을 따른다(파일이 사라졌거나 깨진 경우).
    #[test]
    fn session_start_degrades_when_the_recorded_path_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Path(tmp.path().join("gone.json").display().to_string()),
        );

        let (calls, _) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());

        assert_eq!(restore_command(&calls).unwrap(), "claude -r sess-1");
    }

    /// R4 (6) — 전역 session-end 는 기록에 **종료 표시**를 하고 유예 뒤 sweep 이
    /// 회수한다. 즉시 삭제하지 않는 이유는 실측된 닫은 탭 복원 경로 때문이다:
    /// 탭을 닫으면 이 훅이 정상 발화하는데, 곧바로 Ctrl+Shift+T 로 되살아나는
    /// 세션이 기록을 다시 필요로 한다(`profile_attach` 모듈 doc "수명").
    /// 호출부(`handle_claude_hook`)는 checklist 라운드 정리와 같은 자리에서 부른다.
    #[test]
    fn session_end_marks_the_record_ended_but_keeps_it_for_a_closed_tab_restore() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "probe", r#"{"env":{"A":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("probe".into()),
        );
        profile_attach::mark_ended(Some(tmp.path()), "sess-1");

        // 되살아난 세션의 session-start 가 기록으로 프로필을 복구할 수 있어야 한다.
        let (calls, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        assert!(plan.profile_file.is_some());
        assert!(restore_command(&calls).unwrap().contains("--settings"));
    }

    /// R2 (7) — 이름은 매번 다시 해석한다. 부착 후 등록 내용을 갱신하면 다음
    /// session-start 가 만드는 generated 파일이 갱신본이어야 한다.
    #[test]
    fn session_start_reresolves_names_against_the_current_registry() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "reviewer", r#"{"env":{"VERSION":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("reviewer".into()),
        );

        let (_, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        let generated = std::path::PathBuf::from(plan.profile_file.unwrap());
        let v1: Value =
            serde_json::from_str(&std::fs::read_to_string(&generated).unwrap()).unwrap();
        assert_eq!(v1["env"]["VERSION"], "1");

        register_profile(tmp.path(), "reviewer", r#"{"env":{"VERSION":"2"}}"#);
        let (_, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        let generated = std::path::PathBuf::from(plan.profile_file.unwrap());
        let v2: Value =
            serde_json::from_str(&std::fs::read_to_string(&generated).unwrap()).unwrap();
        assert_eq!(v2["env"]["VERSION"], "2");
    }

    /// R2 보강 (8) — reboot 경쟁 회귀. reboot 이 부착 기록을 쓴 직후 Ctrl+C 가
    /// session-end 를 발화시켜 그 기록을 지우지만, 이어지는 session-start 의
    /// re-stamp 가 복구한다. 이 재기록이 없으면 **모든 reboot 이 기록을 영구
    /// 소실**시켜 다음 앱 재시작에서 프로필이 조용히 빠진다.
    #[test]
    fn reboot_then_session_end_then_session_start_keeps_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "reviewer", r#"{"env":{"A":"1"}}"#);

        // 1) reboot 이 meta + 기록을 부착.
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("reviewer".into()),
        );

        // 2) Ctrl+C 로 죽은 claude 가 session-end 를 발화 — 기록이 지워진다.
        //    같은 이벤트가 프로필 meta 는 건드리지 않는다는 것도 함께 고정한다.
        let end_calls =
            apply_hook("session-end", 7, Some("sess-1"), None, &test_translator()).unwrap();
        assert!(!end_calls.iter().any(|c| matches!(
            c,
            HostCall::MetaUnset { key, .. }
                if *key == crate::reboot::PROFILE_NAMES_META_KEY
                    || *key == crate::reboot::PROFILE_META_KEY
        )));
        profile_attach::mark_ended(Some(tmp.path()), "sess-1");

        // 3) resume 이 session-start 를 발화 — meta 는 살아 있으므로 그 값으로
        //    프로필을 확정하고 기록을 다시 찍는다.
        let (calls, plan) = session_start_calls(7, "sess-1", &names_meta("reviewer"), tmp.path());
        if let Some(record) = &plan.restamp {
            profile_attach::store(Some(tmp.path()), "sess-1", record);
        }

        assert_eq!(
            profile_attach::load(Some(tmp.path()), "sess-1"),
            Some(AttachRecord::Names("reviewer".into())),
            "session-start re-stamp 가 없으면 모든 reboot 이 기록을 영구 소실시킨다"
        );
        assert!(restore_command(&calls).unwrap().contains("--settings"));
    }

    /// (9) — 프로필과 게이트는 이름 평면을 공유한다. 같은 이름이 프로필에서
    /// 게이트로 재등록되면 다음 복원은 **새 정의**로 재해석해야 한다.
    #[test]
    fn session_start_reinterprets_a_name_that_changed_from_profile_to_gate() {
        let tmp = tempfile::tempdir().unwrap();
        register_profile(tmp.path(), "shared", r#"{"env":{"A":"1"}}"#);
        profile_attach::store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("shared".into()),
        );

        let (_, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        let as_profile: Value =
            serde_json::from_str(&std::fs::read_to_string(plan.profile_file.unwrap()).unwrap())
                .unwrap();
        assert_eq!(as_profile["env"]["A"], "1");
        assert!(as_profile.get("hooks").is_none());

        crate::profile::unregister(Some(tmp.path()), "shared").unwrap();
        register_gate(tmp.path(), "shared");

        let (calls, plan) = session_start_calls(7, "sess-1", &no_meta(), tmp.path());
        let as_gate: Value =
            serde_json::from_str(&std::fs::read_to_string(plan.profile_file.unwrap()).unwrap())
                .unwrap();
        assert!(
            as_gate["hooks"]["Stop"].is_array(),
            "게이트로 재등록됐으면 Stop 훅 조각으로 해석돼야 한다: {as_gate}"
        );
        // 게이트 이름으로 부착한 것도 같은 경로로 복원된다.
        assert!(restore_command(&calls).unwrap().contains("--settings"));
    }

    #[test]
    fn unknown_event_returns_invalid_params() {
        let err = apply_hook("bogus", 100, None, None, &test_translator()).unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("bogus"));
    }

    #[test]
    fn resolve_surface_id_prefers_explicit_param() {
        let tr = test_translator();
        assert_eq!(
            resolve_surface_id(&json!({ "surface": 42 }), &tr).unwrap(),
            42
        );
        assert_eq!(
            resolve_surface_id(&json!({ "surface_id": 7 }), &tr).unwrap(),
            7
        );
    }

    #[test]
    fn telemetry_session_start_marks_then_stop_emits_wall_time() {
        let mut state = ClaudeState::default();
        let started = telemetry_for_hook(&mut state, "session-start", 7, None, 1_000);
        assert!(started.is_empty(), "session-start emits no host calls");
        let stopped = telemetry_for_hook(&mut state, "stop", 7, None, 5_000);
        assert_eq!(
            stopped,
            vec![HostCall::TelemetryRecord {
                metric: "wall_time_ms",
                value: 4_000.0,
                surface_id: 7,
            }]
        );
        // 두번째 stop 은 start 가 없으므로 발행 안 함.
        let again = telemetry_for_hook(&mut state, "stop", 7, None, 9_000);
        assert!(again.is_empty());
    }

    #[test]
    fn telemetry_session_end_also_emits_wall_time() {
        let mut state = ClaudeState::default();
        telemetry_for_hook(&mut state, "session-start", 1, None, 100);
        let calls = telemetry_for_hook(&mut state, "session-end", 1, None, 250);
        assert_eq!(
            calls,
            vec![HostCall::TelemetryRecord {
                metric: "wall_time_ms",
                value: 150.0,
                surface_id: 1,
            }]
        );
    }

    #[test]
    fn telemetry_notification_extracts_tokens() {
        let mut state = ClaudeState::default();
        let calls = telemetry_for_hook(
            &mut state,
            "notification",
            42,
            Some("Claude used tokens: 12345 in this turn"),
            0,
        );
        assert_eq!(
            calls,
            vec![HostCall::TelemetryRecord {
                metric: "input_tokens",
                value: 12345.0,
                surface_id: 42,
            }]
        );
    }

    #[test]
    fn telemetry_notification_no_match_no_record() {
        let mut state = ClaudeState::default();
        let calls = telemetry_for_hook(&mut state, "notification", 1, Some("approval needed"), 0);
        assert!(calls.is_empty());
    }

    #[test]
    fn extract_tokens_variants() {
        assert_eq!(extract_tokens("tokens: 99"), Some(99));
        assert_eq!(extract_tokens("token:5"), Some(5));
        assert_eq!(extract_tokens("Tokens:   1000  used"), Some(1000));
        assert_eq!(extract_tokens("notokens: 5"), None);
        assert_eq!(extract_tokens("tokens 5"), None); // 콜론 없음
        assert_eq!(extract_tokens("xtoken: 5"), None); // 워드 경계 위반
    }

    /// 예전 이 테스트는 `TASTY_SURFACE_ID` 가 있으면 `return` 으로 자기를 건너뛰었다.
    /// 그 변수는 우리 실행 환경에 **항상** 있으므로 초록은 "위반 없음" 이 아니라
    /// **"0회 실행"** 이었다. env 를 인자로 받는 판정부를 쓰면 건너뛸 이유가 없다.
    #[test]
    fn resolve_surface_id_missing_and_no_env_is_invalid_params() {
        let err = resolve_surface_id_from(&json!({}), None, &test_translator()).unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn resolve_surface_id_falls_back_to_env_when_the_param_is_absent() {
        let sid = resolve_surface_id_from(&json!({}), Some("42"), &test_translator()).unwrap();
        assert_eq!(sid, 42);
    }

    /// `u32` 범위 밖 값은 잘라서 받지 않는다. 자르면 `5_000_000_000` → `705_032_704`
    /// 로 **실재할 수 있는 다른 surface** 를 가리키게 된다(고치기 전 실측값이다).
    /// 이 형태는 자기-배달보다 나쁘다 — 남의 터미널로 가고, 되읽기로도 안 잡힌다.
    #[test]
    fn a_surface_beyond_u32_is_rejected_not_truncated() {
        let big = serde_json::json!({ "surface": 5_000_000_000u64 });
        let err = resolve_surface_id_from(&big, Some("7"), &test_translator())
            .expect_err("u32 를 넘는 값은 오류여야 한다");
        assert_eq!(err.code, -32602);
        // 대조: 범위 안의 값은 그대로 통과한다(문이 닫힌 것이 아니다).
        let ok = resolve_surface_id_from(
            &serde_json::json!({ "surface": 4_294_967_295u64 }),
            None,
            &test_translator(),
        )
        .expect("u32::MAX 는 유효한 값이다");
        assert_eq!(ok, u32::MAX);
    }

    /// 값이 왔는데 숫자가 아니면 **거부**한다 — env 폴백으로 넘기지 않는다.
    /// 넘기면 호출자 자신에게 배달된다. `--surface conductor` 로 실제로 그렇게 잃은 적이 있다.
    #[test]
    fn a_non_numeric_surface_is_rejected_and_does_not_fall_back_to_env() {
        for bad in [
            json!({ "surface": "conductor" }),
            json!({ "surface_id": "7x" }),
        ] {
            let err = resolve_surface_id_from(&bad, Some("42"), &test_translator())
                .expect_err("숫자가 아닌 surface 는 거부해야 한다");
            assert_eq!(err.code, -32602, "params={bad}");
        }
    }

    // ── error dedupe 초기화 배선 (disable/reset_dedupe/is_enabled 재배선) ──

    #[test]
    fn is_new_turn_event_matches_active_transition_only() {
        for event in ["prompt-submit", "session-start", "active"] {
            assert!(is_new_turn_event(event), "{event} 은 새 턴 신호여야 함");
        }
        for event in [
            "stop",
            "subagent-stop",
            "notification",
            "session-end",
            "pre-tool-use",
            "post-tool-use",
        ] {
            assert!(!is_new_turn_event(event), "{event} 은 새 턴 신호가 아님");
        }
    }

    #[test]
    fn reset_dedupe_if_enabled_clears_tracked_surface() {
        let scanner = Arc::new(Mutex::new(ErrorScanner::new()));
        scanner
            .lock()
            .unwrap()
            .enable(7, crate::error_scan::ScanTarget::TopLevel);
        scanner
            .lock()
            .unwrap()
            .seed_dedupe_for_test(7, "API Error: boom");

        reset_dedupe_if_enabled(&scanner, 7);

        assert!(
            !scanner.lock().unwrap().has_dedupe_state(7),
            "추적 중인 surface 는 dedupe 가 초기화돼야 함"
        );
    }

    #[test]
    fn reset_dedupe_if_enabled_skips_untracked_surface() {
        // release 이후처럼 error_scan 이 더 이상 추적하지 않는 surface — hook
        // 이벤트가 와도 조용히 무시해야 한다. dedupe 상태가 (다른 경로로) 이미
        // 있더라도 is_enabled 가 false 면 건드리지 않아야 함을 직접 확인한다.
        let scanner = Arc::new(Mutex::new(ErrorScanner::new()));
        scanner
            .lock()
            .unwrap()
            .seed_dedupe_for_test(99, "unrelated");

        reset_dedupe_if_enabled(&scanner, 99);

        assert!(!scanner.lock().unwrap().is_enabled(99));
        assert!(
            scanner.lock().unwrap().has_dedupe_state(99),
            "추적 대상이 아니면 dedupe 상태를 건드리지 않아야 함"
        );
    }
}
