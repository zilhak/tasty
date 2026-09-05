//! `agent_stream.*` IPC 핸들러.
//!
//! 등록은 **surface_id 를 명시적으로 지정**하는 것만 지원한다 — "전부 watch" 와일드카드는
//! 두지 않는다. 이유는 두 가지다:
//!
//! - 대상을 ID 로 직접 지정한다는 tasty 의 포커스 독립 원칙(`docs/identity.md` §2.3)과
//!   같은 결이다. 와일드카드는 "지금 떠 있는 것들" 이라는 암묵적·시점 의존 대상 집합을
//!   만든다.
//! - transcript 는 대화 전문이다. 요청하지 않은 세션까지 자동으로 tail 하면 중계 범위가
//!   호출자의 의도를 넘는다. 여러 대상이 필요하면 surface 마다 명시적으로 등록한다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tasty_plugin_sdk::{IpcMethodError, i18n::Translator};

use crate::registry::{
    DEFAULT_TURN_TIMEOUT_SECS, MAX_TURN_TIMEOUT_SECS, MIN_TURN_TIMEOUT_SECS, StreamRegistry,
    TurnError, new_watch,
};
use crate::resolve::{self, CLAUDE_SESSION_META_KEY, HostCall, ResolveError};
use crate::sse::server::{self, SseServer};
use crate::sse::{ConfigError, ServeConfig};

/// `poll` 의 기본/최대 반환 개수.
const POLL_DEFAULT_LIMIT: u64 = 100;
const POLL_MAX_LIMIT: u64 = 1000;

/// `request_id` 의 바이트 상한. 웹훅 `${body.request_id}` 는 외부 입력이라 상한이 없으면
/// 거대한 값이 TurnState 에 저장돼 그 턴의 모든 이벤트(SSE·poll)에 복제되는 증폭이 된다.
/// FE correlation 토큰(UUID·nanoid·복합키)에 넉넉한 512 바이트에서 자르지 않고 거부한다.
const MAX_REQUEST_ID_LEN: usize = 512;

type Shared = Arc<Mutex<StreamRegistry>>;

/// mutex poisoning 은 tail 스레드가 패닉했다는 뜻 — 조용히 성공한 척하지 않는다.
fn lock<'a>(
    registry: &'a Shared,
    tr: &Translator,
) -> Result<std::sync::MutexGuard<'a, StreamRegistry>, IpcMethodError> {
    registry.lock().map_err(|e| {
        IpcMethodError::new(tr.t_replace(
            "agent_stream.error.registry_poisoned",
            "{detail}",
            &e.to_string(),
        ))
    })
}

/// params 에서 대상 surface id 를 읽는다.
///
/// **"키가 없다" 와 "값이 surface id 일 수 없다" 를 가른다.** 예전에는 둘 다
/// `missing_surface` 로 답했다 — 즉 `surface: 999999999999` 처럼 **값을 실어 보낸**
/// 요청에 "대상을 안 줬다" 고 답했다. 호출자는 자기가 준 값을 서버가 못 본 줄 알고
/// 같은 값을 다시 보낸다. 값이 있는데 없다고 하는 답은 조용한 오답이다.
///
/// 여기서 판정하는 것은 **형식**뿐이다 — 그 id 의 surface 가 실제로 사는지는 별개
/// 질문이고, 그쪽은 각 핸들러가 host 에 묻는다([`require_live_surface`]).
fn require_surface(params: &Value, tr: &Translator) -> Result<u32, IpcMethodError> {
    let Some(raw) = params.get("surface").or_else(|| params.get("surface_id")) else {
        return Err(IpcMethodError::invalid_params(
            tr.t("agent_stream.error.missing_surface"),
        ));
    };
    raw.as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| {
            IpcMethodError::invalid_params(&tr.t_replace(
                "agent_stream.error.invalid_surface",
                "{value}",
                &raw.to_string(),
            ))
        })
}

/// 레지스트리가 "이 surface 는 안 보고 있다" 고 답했을 때, 그것이 **없는 surface** 인지
/// 확인한다.
///
/// 없는 surface 에 "보고 있지 않다" 고 답하면 살아 있는 미watch surface 와 **완전히 같은
/// 문장**이 나온다(실측: 존재하지 않는 424242 와 살아 있는 1 이 같은 답). 호출자는
/// `watch` 를 부르면 되는 줄 알고 다시 부르고, 거기서야 다른 이유로 실패한다.
///
/// **문구는 한 벌뿐이다** — [`tasty_utils::target::unowned_target_message`] 가 정본이고
/// 호스트도 같은 함수를 부른다. 이 문장을 여기서 다시 쓰던 동안 실제로 어긋나 있었다:
/// 호스트는 `(named by '<메서드>')` 에 **요청의 메서드 이름**을 넣는데 복제본은 인자
/// 이름(`'surface'`)을 박아 두어 문장의 뜻이 달랐다. lang 파일에 두는 것도 같은 이유로
/// 답이 아니다 — 로케일마다 바이트가 갈리면 이 문구를 문자열로 가르는 소비자가 깨진다.
/// 그래서 바이트 동일이 **테스트의 단언이 아니라 호출 구조**로 보장된다.
///
/// `method` 는 **호출자가 부른 메서드**(`agent_stream.turn_start` 등)다 — 생존을 되묻느라
/// 내부적으로 부른 host 호출의 이름이 아니다. 호출자가 이름으로 지목한 것이 그쪽이다.
///
/// 판정은 **좁게 틀린다**: host 호출이 실패하면 `surface_exists` 가 `true` 로 떨어져
/// 예전 문장("보고 있지 않다")으로 돌아간다. 살아 있는 surface 를 "없다" 고 말하지 않는다.
fn require_live_surface<H: HostCall>(
    host: &H,
    surface_id: u32,
    method: &str,
    fallback: IpcMethodError,
) -> IpcMethodError {
    if crate::resolve::surface_exists(host, surface_id) {
        return fallback;
    }
    IpcMethodError::new(tasty_utils::target::unowned_target_message(
        "surface",
        u64::from(surface_id),
        method,
    ))
}

/// `resolve` 가 낸 실패를 IPC 에러로 옮긴다.
///
/// `surface_id`·`method` 를 받는 이유는 마지막 갈래 하나 때문이다: 호스트가 "그런 대상은
/// 없다" 고 거절한 것을 **그대로 실어 보내면 괄호 안이 내부 호출 이름**(`surface.meta.get`)
/// 이 된다. 그 이름은 호출자가 지목한 적이 없다 — 지목한 것은 이 메서드다. 실측
/// (2026-09-05): `agent_stream.watch` 에 없는 id 를 주면 `(named by 'surface.meta.get')`
/// 이 돌아왔고, 같은 사정에서 `turn_start`·`unwatch` 는 자기 이름을 답한다. 한 사정에
/// 세 가지 이름이 나오면 호출자는 그 괄호를 못 읽는다.
///
/// 그 외의 host 실패는 그대로 넘긴다 — 거기 담긴 사유가 유일한 정보다.
fn resolve_error_message(
    tr: &Translator,
    err: &ResolveError,
    surface_id: u32,
    method: &str,
) -> IpcMethodError {
    match err {
        ResolveError::NoSessionMeta { surface_id } => IpcMethodError::new(
            tr.t_replace(
                "agent_stream.error.no_session_meta",
                "{surface}",
                &surface_id.to_string(),
            )
            .replace("{key}", CLAUDE_SESSION_META_KEY),
        ),
        ResolveError::TranscriptRootMissing => {
            IpcMethodError::new(tr.t("agent_stream.error.transcript_root_missing"))
        }
        ResolveError::TranscriptNotFound { session_id } => IpcMethodError::new(tr.t_replace(
            "agent_stream.error.transcript_not_found",
            "{session}",
            session_id,
        )),
        ResolveError::HostCall { message } => {
            if tasty_utils::target::says_no_live_target(message, "surface", u64::from(surface_id)) {
                IpcMethodError::new(tasty_utils::target::unowned_target_message(
                    "surface",
                    u64::from(surface_id),
                    method,
                ))
            } else {
                IpcMethodError::new(message.clone())
            }
        }
    }
}

/// `agent_stream.watch` — 대상 surface 의 세션 transcript tail 을 시작한다.
///
/// 세션 id meta 가 없으면 **거부한다**. 어떤 파일을 볼지 결정할 수 없는데도 등록을
/// 받아주면 호출자는 스트림이 붙었다고 믿은 채 아무것도 못 받는다(조용한 무동작 금지).
///
/// 반대로 세션 id 는 알지만 파일이 아직 없는 것은 정상 상태다 — 세션 시작 직후의 race
/// 이므로 `awaiting_transcript` 로 등록하고 tail 루프가 계속 재해석한다.
pub fn handle_watch<H: HostCall>(
    host: &H,
    registry: &Shared,
    tr: &Translator,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface(&params, tr)?;
    let from_start = params
        .get("from_start")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let session_id = resolve::session_id_for_surface(host, surface_id)
        .map_err(|e| resolve_error_message(tr, &e, surface_id, "agent_stream.watch"))?;

    let transcript = match resolve::transcript_path(&session_id) {
        Ok(path) => Some(path),
        Err(ResolveError::TranscriptNotFound { .. }) => None,
        Err(e) => {
            return Err(resolve_error_message(
                tr,
                &e,
                surface_id,
                "agent_stream.watch",
            ));
        }
    };

    let mut reg = lock(registry, tr)?;
    let replaced = reg.insert(new_watch(
        surface_id,
        session_id.clone(),
        transcript.clone().unwrap_or_default(),
        from_start,
    ));
    let offset = reg.watch_mut(surface_id).map(|w| w.offset()).unwrap_or(0);
    reg.save_if_dirty();
    Ok(json!({
        "surface_id": surface_id,
        "session_id": session_id,
        "transcript": transcript.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "offset": offset,
        "status": if transcript.is_some() { "tailing" } else { "awaiting_transcript" },
        "replaced": replaced,
    }))
}

/// 웹훅 값 슬롯으로 넘어오는 `request_id` 를 읽는다. `${body.request_id}` 는 전체
/// 플레이스홀더면 JSON 타입을 보존하므로(문자열이면 문자열, 숫자면 숫자) 둘 다 받아
/// 문자열로 정규화한다. 없거나 빈 값이면 거부한다 — correlation id 는 요청자만 알 수 있는
/// 값이라(웹훅 응답은 고정 ACK) 매칭의 유일한 성립 경로다. 자체 생성하면 FE 가 그 값을
/// 알 방법이 없다.
fn require_request_id(params: &Value, tr: &Translator) -> Result<String, IpcMethodError> {
    let raw = match params.get("request_id") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(IpcMethodError::invalid_params(
            tr.t("agent_stream.error.missing_request_id"),
        ));
    }
    if trimmed.len() > MAX_REQUEST_ID_LEN {
        return Err(IpcMethodError::invalid_params(&tr.t_replace(
            "agent_stream.error.request_id_too_long",
            "{max}",
            &MAX_REQUEST_ID_LEN.to_string(),
        )));
    }
    Ok(trimmed.to_string())
}

/// `agent_stream.turn_start` — surface 에 correlation 턴을 연다.
///
/// 웹훅 IpcSequence 의 **첫 스텝**으로 쓴다(두 번째가 `claude.tell`). 이 호출이 먼저
/// 끝나야 그 사이 도착하는 transcript 이벤트가 누락 없이 `request_id` 로 태깅된다 —
/// `execute_sequence` 가 스텝을 **순차** 실행하므로 순서가 보장된다(`src/hook_handler/exec.rs`).
///
/// 턴은 그 surface 의 다음 `turn_end`(정상 종료·취소·오류·해제·세션 소멸) 가 닫는다.
/// claude-idle 훅을 구독하지 않는 이유: transcript 가 이미 그 신호를 만들고(ADR-0093),
/// 훅 구독은 claude plugin 이 활성일 때만 성립하는 의존을 새로 만들기 때문이다.
pub fn handle_turn_start<H: HostCall>(
    host: &H,
    registry: &Shared,
    tr: &Translator,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface(&params, tr)?;
    let request_id = require_request_id(&params, tr)?;
    let timeout_secs = params
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_TURN_TIMEOUT_SECS)
        .clamp(MIN_TURN_TIMEOUT_SECS, MAX_TURN_TIMEOUT_SECS);

    let mut reg = lock(registry, tr)?;
    reg.start_turn(
        surface_id,
        request_id.clone(),
        Duration::from_secs(timeout_secs),
    )
    .map_err(|e| {
        let msg = turn_error_message(tr, surface_id, &e);
        // "안 보고 있다" 는 **없는 surface** 에도 같은 말이 된다 — 그 자리에서만 host 에
        // 되묻는다(정상 경로엔 왕복이 붙지 않는다).
        match e {
            TurnError::NotWatched => {
                require_live_surface(host, surface_id, "agent_stream.turn_start", msg)
            }
            TurnError::AlreadyOpen { .. } => msg,
        }
    })?;
    Ok(json!({
        "surface_id": surface_id,
        "request_id": request_id,
        "timeout_secs": timeout_secs,
        "turn_open": true,
    }))
}

fn turn_error_message(tr: &Translator, surface_id: u32, err: &TurnError) -> IpcMethodError {
    match err {
        TurnError::NotWatched => IpcMethodError::new(tr.t_replace(
            "agent_stream.error.turn_not_watched",
            "{surface}",
            &surface_id.to_string(),
        )),
        TurnError::AlreadyOpen { request_id } => IpcMethodError::new(
            tr.t_replace(
                "agent_stream.error.turn_already_open",
                "{surface}",
                &surface_id.to_string(),
            )
            .replace("{request}", request_id),
        ),
    }
}

/// `agent_stream.unwatch` — tail 을 멈추고 종료 이벤트를 남긴다.
pub fn handle_unwatch<H: HostCall>(
    host: &H,
    registry: &Shared,
    tr: &Translator,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface(&params, tr)?;
    let mut reg = lock(registry, tr)?;
    if !reg.remove(surface_id, crate::record::REASON_UNWATCHED) {
        // 보고 있던 surface 는 죽어 있어도 여기까지 안 온다(위 `remove` 가 참) — 즉
        // 죽은 대상의 뒷정리를 막지 않는다. 생존을 되묻는 것은 **레지스트리에 없을 때**뿐이다.
        let fallback = IpcMethodError::new(tr.t_replace(
            "agent_stream.error.not_watched",
            "{surface}",
            &surface_id.to_string(),
        ));
        return Err(require_live_surface(
            host,
            surface_id,
            "agent_stream.unwatch",
            fallback,
        ));
    }
    reg.save_if_dirty();
    Ok(json!({ "surface_id": surface_id, "unwatched": true }))
}

/// `agent_stream.list` — 현재 tail 중인 대상 전부. 포커스와 무관하게 전 대상을 돌려준다.
pub fn handle_list(registry: &Shared, tr: &Translator) -> Result<Value, IpcMethodError> {
    Ok(lock(registry, tr)?.list_json())
}

/// `agent_stream.poll` — seq 커서로 수집 이벤트를 읽는다(비파괴).
pub fn handle_poll(
    registry: &Shared,
    tr: &Translator,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let filter_surface = params
        .get("filter_surface")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());
    let after_seq = params
        .get("after_seq")
        .and_then(Value::as_i64)
        .map(|v| v.max(0) as u64)
        .unwrap_or(0);
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(POLL_DEFAULT_LIMIT)
        .clamp(1, POLL_MAX_LIMIT) as usize;
    Ok(lock(registry, tr)?.poll_json(filter_surface, after_seq, limit))
}

/// `agent_stream.serve` — SSE 엔드포인트를 연다.
///
/// 이미 떠 있으면 **끄고 새 설정으로 다시 연다**(`replaced: true`). 포트/토큰을 바꾸려고
/// 별도 명령을 쓰게 만들 이유가 없고, "요청한 설정으로 열려 있다" 는 결과가 호출 횟수와
/// 무관하게 같아진다.
///
/// # 불변: 실행 중인 엔드포인트 ↔ 영속 스냅샷은 어긋나지 않는다
///
/// 이 함수(와 [`handle_serve_stop`])가 반환한 뒤, 스냅샷의 `serve` 절은 **이 프로세스에서
/// 실제로 떠 있는(혹은 떠 있지 않은) 엔드포인트를 그대로** 기술한다. 어긋나면 다음 강제
/// 재시작 때 plugin 기동 시의 `restore_endpoint` 가 사용자가 닫혔다고 믿는
/// 주소를 열거나, 열려 있다고 믿는 주소를 열지 않는다 — 대화 전문이 나가는 채널에서
/// 그것은 ADR-0100 결정 1(명시적으로 켤 때만 뜬다)과 정면으로 어긋난다.
///
/// 불변을 지키는 규칙은 둘이다.
///
/// 1. **아직 아무것도 바꾸지 않았으면 실패를 그대로 올린다.** 첫 레지스트리 락이
///    poisoned 면 옛 리스너도 스냅샷도 손대기 전이라, 에러로 빠지는 것이 곧 정합이다.
/// 2. **이미 바꿨으면 기록은 실패하지 않는다.** 리스너를 내렸거나 새로 띄운 뒤의
///    스냅샷 기록은 [`persist_serve_config`] 로 하며, 그 함수는 poisoned 락을
///    복구해서라도 쓴다. 여기서 `?` 로 빠지면 "실행 주소 ≠ 스냅샷" 이 그대로 남는다.
pub fn handle_serve(
    registry: &Shared,
    server: &mut Option<SseServer>,
    tr: &Translator,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let config = serve_config_from(&params, tr)?;
    let replaced = server.is_some();
    // 옛 리스너를 내리기 **전에** 레지스트리 락을 한 번 잡아 둔다. 순서를 뒤집으면
    // 락이 poisoned 인 경우 옛 엔드포인트만 닫힌 채 `?` 로 빠져나가, 스냅샷에는 옛 설정이
    // 남는다 — 다음 재시작이 사용자가 닫혔다고 믿는 엔드포인트를 다시 연다.
    let hub = lock(registry, tr)?.hub();
    // 새로 bind 하기 전에 옛 리스너를 반드시 내린다 — 같은 포트로 재기동하는 흔한 경우에
    // 남겨두면 "주소가 이미 사용 중" 으로 실패한다.
    if let Some(mut old) = server.take() {
        old.shutdown();
    }
    let started = match server::start(config.clone(), hub, registry.clone()) {
        Ok(started) => started,
        Err(e) => {
            // 옛 리스너는 이미 내려갔고 새 bind 는 실패했다 — 런타임 상태는 "닫힘" 이다.
            // 스냅샷에 옛 설정을 남겨두면 다음 강제 재시작/`enable` 때
            // `restore_endpoint` 가 사용자가 닫혔다고 믿는 엔드포인트를 조용히 다시 연다.
            // 대화 전문이 나가는 채널에서 "닫힌 줄 알았는데 열림" 은 ADR-0100 결정 1
            // (명시적으로 켤 때만 뜬다)과 정면으로 어긋난다.
            persist_serve_config(registry, None);
            return Err(bind_failed_message(tr, &config, &e));
        }
    };
    let info = started.to_json();
    *server = Some(started);
    // 여기서부터는 실패로 빠질 수 없다 — 새 엔드포인트가 이미 떠 있으므로 스냅샷도
    // 반드시 그 주소를 가리켜야 한다(위 불변 규칙 2).
    persist_serve_config(registry, Some(config));
    let mut info = info;
    if let Some(map) = info.as_object_mut() {
        map.insert("replaced".into(), Value::from(replaced));
    }
    Ok(info)
}

fn bind_failed_message(tr: &Translator, config: &ServeConfig, detail: &str) -> IpcMethodError {
    IpcMethodError::new(
        tr.t_replace(
            "agent_stream.error.serve_bind_failed",
            "{addr}",
            &format!("{}:{}", config.bind, config.port),
        )
        .replace("{detail}", detail),
    )
}

/// 스냅샷의 `serve` 절을 지금의 런타임 상태로 맞춘다 — **실패하지 않는다.**
///
/// 호출 시점에는 리스너를 이미 내렸거나 새로 띄운 뒤다. 그 상태를 기록하지 못하고
/// 빠져나가면 "실행 주소 ≠ 스냅샷" 이 남아, 다음 재시작이 엉뚱한 주소를 연다
/// (`handle_serve` 의 불변 참고). 그래서 락이 poisoned 여도 `PoisonError::into_inner`
/// 로 복구해 기록한다.
///
/// 오염된 데이터를 쓰게 되지 않는가 — 여기서 바꾸는 `serve` 절은 **요청 파라미터에서
/// 검증을 마치고 온 값**이라 패닉한 스레드가 만지던 것과 무관하다. 함께 직렬화되는
/// 나머지(watch 목록·offset·`next_seq`)는 패닉으로 찢어지지 않는 Rust 값이고, 최악이라야
/// 한 tick 낡은 offset 이나 앞선 `seq` 인데 둘 다 tail 의 at-least-once 재개(ADR-0093)가
/// 이미 감당하는 범위다. 반면 기록을 건너뛰면 손실이 확정적이다.
fn persist_serve_config(registry: &Shared, config: Option<ServeConfig>) {
    let mut reg = match registry.lock() {
        Ok(reg) => reg,
        Err(poisoned) => {
            tracing::warn!(
                "agent-stream: the registry lock is poisoned (the tail thread panicked) — \
                 recovering it to record the SSE endpoint state, otherwise a restart would \
                 reopen an address that is no longer the live one"
            );
            poisoned.into_inner()
        }
    };
    reg.set_serve_config(config);
    reg.save_if_dirty();
}

/// `agent_stream.serve_stop` — 엔드포인트를 닫고 열린 구독을 정리한다.
///
/// [`handle_serve`] 와 같은 불변을 지킨다: 리스너를 내린 **뒤**의 스냅샷 기록은 실패로
/// 빠질 수 없다. 빠지면 사용자가 닫은 엔드포인트가 스냅샷에 남아 다음 재시작에 되살아난다.
pub fn handle_serve_stop(
    registry: &Shared,
    server: &mut Option<SseServer>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let Some(mut running) = server.take() else {
        return Err(IpcMethodError::new(
            tr.t("agent_stream.error.serve_not_running"),
        ));
    };
    running.shutdown();
    persist_serve_config(registry, None);
    Ok(json!({ "running": false, "stopped": true }))
}

/// `agent_stream.serve_info` — 엔드포인트 상태와 구독자 통계. 토큰은 싣지 않는다.
pub fn handle_serve_info(server: &Option<SseServer>) -> Result<Value, IpcMethodError> {
    match server {
        Some(running) => Ok(running.to_json()),
        None => Ok(json!({ "running": false })),
    }
}

fn serve_config_from(params: &Value, tr: &Translator) -> Result<ServeConfig, IpcMethodError> {
    // 포트 범위를 벗어난 값은 "지정하지 않음" 으로 접지 않는다 — 그러면 `--port 70000`
    // 이 "포트가 지정되지 않았다" 로 안내되어 원인과 다른 메시지가 나간다.
    let port = match params.get("port").and_then(Value::as_u64) {
        Some(raw) => u16::try_from(raw).map_err(|_| {
            IpcMethodError::invalid_params(&tr.t_replace(
                "agent_stream.error.serve_port_out_of_range",
                "{port}",
                &raw.to_string(),
            ))
        })?,
        // 미지정 — 아래 `validate()` 가 `PortRequired` 로 거른다.
        None => 0,
    };
    let bind = params
        .get("bind")
        .and_then(Value::as_str)
        .unwrap_or(ServeConfig::DEFAULT_BIND)
        .to_string();
    let token = params
        .get("token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let config = ServeConfig { bind, port, token };
    config
        .validate()
        .map_err(|e| config_error_message(tr, &config, e))?;
    Ok(config)
}

fn config_error_message(tr: &Translator, config: &ServeConfig, err: ConfigError) -> IpcMethodError {
    match err {
        ConfigError::PortRequired => {
            IpcMethodError::invalid_params(tr.t("agent_stream.error.serve_port_required"))
        }
        ConfigError::InvalidBind => IpcMethodError::invalid_params(&tr.t_replace(
            "agent_stream.error.serve_invalid_bind",
            "{bind}",
            &config.bind,
        )),
        ConfigError::RemoteBindNeedsToken => IpcMethodError::invalid_params(&tr.t_replace(
            "agent_stream.error.serve_remote_bind_needs_token",
            "{bind}",
            &config.bind,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_plugin_sdk::PluginError;

    struct StubHost {
        session: Option<&'static str>,
        /// `surface.locate` 가 돌려줄 값. 기본은 살아 있음 —
        /// [`live()`] 로 만든다. 없는 surface 를 재는 테스트만 false 를 쓴다.
        exists: bool,
    }

    /// 살아 있는 surface 를 답하는 host. 기존 테스트의 전제를 그대로 유지한다.
    fn live() -> StubHost {
        StubHost {
            session: None,
            exists: true,
        }
    }

    impl HostCall for StubHost {
        fn call(&self, method: &str, _params: Value) -> Result<Value, PluginError> {
            match method {
                "surface.meta.get" => Ok(json!({ "value": self.session })),
                "surface.locate" => Ok(json!({ "exists": self.exists })),
                other => panic!("unexpected host call {other}"),
            }
        }
    }

    fn shared() -> Shared {
        Arc::new(Mutex::new(StreamRegistry::new(None)))
    }

    /// 지금 비어 있는 루프백 포트 하나. 바인드해 번호를 읽고 바로 놓는다.
    /// 임시 포트를 리스너째 예약해 반환한다. 소비자(`handle_serve`)는 port 번호만 받으므로,
    /// 실제 bind 직전에 이 리스너를 drop 해 예약~bind 사이의 TOCTOU 창을 최소화한다
    /// (ADR-0129 형태 B 정방향, `tasty-ssh::reserve_local_port` 와 동형). 그냥 bind 후
    /// 곧바로 놓아 port 번호만 돌려주면, 그 사이 같은 머신의 다른 완주가 포트를 집어가
    /// `serve_bind_failed` 로 확률적 red 가 난다.
    fn reserve_port() -> (std::net::TcpListener, u16) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        (listener, port)
    }

    /// 레지스트리 락을 실제로 poisoned 로 만든다 — 락을 쥔 스레드를 패닉시키는 것이
    /// 유일한 방법이라(std 에 강제 poison API 가 없다) 그대로 재현한다. 이 테스트가
    /// 일부러 낸 패닉이라는 것을 출력에서 알아볼 수 있게 메시지를 남긴다.
    fn poison_registry(registry: &Shared) {
        let target = registry.clone();
        let joined = std::thread::spawn(move || {
            let _guard = target.lock().expect("lock");
            panic!("intentional: poisoning the registry lock for a regression test");
        })
        .join();
        assert!(joined.is_err(), "패닉이 나야 락이 poisoned 가 된다");
        assert!(registry.lock().is_err(), "락이 poisoned 여야 한다");
    }

    fn snapshot_serve(dir: &std::path::Path) -> Value {
        let text = std::fs::read_to_string(dir.join("watches.json")).expect("snapshot");
        serde_json::from_str::<Value>(&text).expect("json")["serve"].clone()
    }

    #[test]
    fn the_snapshot_records_the_closed_endpoint_even_when_the_lock_is_poisoned() {
        // `serve_stop` 은 리스너를 먼저 내리고 나서 스냅샷을 쓴다 — 그 사이에 락이
        // poisoned 면 예전 코드는 `?` 로 빠져 "닫았는데 스냅샷엔 남아 있음" 이 됐다.
        let dir = tempfile::tempdir().expect("tempdir");
        let registry: Shared = Arc::new(Mutex::new(StreamRegistry::new(Some(dir.path()))));
        let tr = Translator::default();
        let mut server = None;

        let (reservation, port) = reserve_port();
        drop(reservation);
        handle_serve(
            &registry,
            &mut server,
            &tr,
            json!({"port": port, "bind": "127.0.0.1"}),
        )
        .expect("the first bind succeeds");
        assert_ne!(snapshot_serve(dir.path()), Value::Null);

        poison_registry(&registry);

        handle_serve_stop(&registry, &mut server, &tr).expect("stopping must not fail");
        assert!(server.is_none(), "런타임은 닫혔다");
        assert_eq!(
            snapshot_serve(dir.path()),
            Value::Null,
            "스냅샷도 닫힘이어야 한다 — 남으면 다음 재시작이 되살린다"
        );
    }

    #[test]
    fn persisting_the_serve_clause_survives_a_poisoned_lock_in_both_directions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry: Shared = Arc::new(Mutex::new(StreamRegistry::new(Some(dir.path()))));
        poison_registry(&registry);

        let config = ServeConfig {
            bind: "127.0.0.1".into(),
            port: 8787,
            token: Some("t".into()),
        };
        persist_serve_config(&registry, Some(config));
        assert_eq!(snapshot_serve(dir.path())["port"], json!(8787));

        persist_serve_config(&registry, None);
        assert_eq!(snapshot_serve(dir.path()), Value::Null);
    }

    #[test]
    fn a_poisoned_lock_before_anything_changes_is_reported_without_touching_the_endpoint() {
        // 아직 아무것도 안 바꾼 시점의 실패는 그대로 올린다 — 그때는 에러를 내는 쪽이
        // 정합이다(옛 엔드포인트도 스냅샷도 손대지 않았다).
        let dir = tempfile::tempdir().expect("tempdir");
        let registry: Shared = Arc::new(Mutex::new(StreamRegistry::new(Some(dir.path()))));
        let tr = Translator::default();
        let mut server = None;

        let (reservation, first_port) = reserve_port();
        drop(reservation);
        handle_serve(
            &registry,
            &mut server,
            &tr,
            json!({"port": first_port, "bind": "127.0.0.1"}),
        )
        .expect("the first bind succeeds");

        poison_registry(&registry);

        // poison 으로 bind 도달 전에 실패하므로 port 는 쓰이지 않는다 — 예약을 잡지 않는다.
        let err = handle_serve(
            &registry,
            &mut server,
            &tr,
            json!({"port": reserve_port().1, "bind": "127.0.0.1"}),
        )
        .expect_err("a poisoned registry must be reported, not swallowed");
        assert!(
            err.message.contains("registry_poisoned") || err.message.contains("poisoned"),
            "{}",
            err.message
        );

        // 옛 엔드포인트는 그대로 떠 있고 스냅샷도 그 주소를 가리킨다.
        let info = server
            .as_ref()
            .expect("the old endpoint stays up")
            .to_json();
        assert_eq!(info["port"], json!(first_port), "{info}");
        assert_eq!(snapshot_serve(dir.path())["port"], json!(first_port));

        server.take().expect("server").shutdown();
    }

    #[test]
    fn a_failed_rebind_clears_the_persisted_endpoint_so_a_restart_does_not_reopen_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry: Shared = Arc::new(Mutex::new(StreamRegistry::new(Some(dir.path()))));
        let tr = Translator::default();
        let mut server = None;

        let (reservation, port) = reserve_port();
        drop(reservation);
        handle_serve(
            &registry,
            &mut server,
            &tr,
            json!({"port": port, "bind": "127.0.0.1"}),
        )
        .expect("the first bind succeeds");
        assert!(registry.lock().expect("lock").serve_config().is_some());

        // 이 주소는 이 호스트의 것이 아니다(TEST-NET-3) — bind 가 반드시 실패한다.
        let err = handle_serve(
            &registry,
            &mut server,
            &tr,
            json!({"port": reserve_port().1, "bind": "192.0.2.1", "token": "t"}),
        )
        .expect_err("binding a foreign address must fail");
        assert!(
            err.message.contains("serve_bind_failed") || err.message.contains("192.0.2.1"),
            "{}",
            err.message
        );

        // 런타임도 스냅샷도 "닫힘" 이어야 한다 — 다음 재시작이 옛 설정을 되살리면
        // 사용자가 닫혔다고 믿는 채널로 대화 전문이 다시 나간다.
        assert!(server.is_none(), "the runtime endpoint is closed");
        assert!(registry.lock().expect("lock").serve_config().is_none());
        let snapshot: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("watches.json")).expect("snapshot"),
        )
        .expect("json");
        assert_eq!(snapshot["serve"], Value::Null, "{snapshot}");
    }

    #[test]
    fn a_port_outside_the_u16_range_reports_its_real_cause() {
        let err = handle_serve(
            &shared(),
            &mut None,
            &Translator::default(),
            json!({"port": 70000}),
        )
        .expect_err("must reject");
        assert_eq!(err.code, -32602);
        assert!(
            err.message.contains("serve_port_out_of_range") || err.message.contains("70000"),
            "{}",
            err.message
        );
        assert!(
            !err.message.contains("serve_port_required"),
            "포트 미지정 안내로 접히면 원인이 가려진다: {}",
            err.message
        );
    }

    #[test]
    fn watch_without_a_target_surface_is_rejected() {
        let host = StubHost {
            session: Some("s"),
            exists: true,
        };
        let err = handle_watch(&host, &shared(), &Translator::default(), json!({}))
            .expect_err("must reject");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("missing_surface") || err.message.contains("--surface"));
    }

    #[test]
    fn watch_without_session_meta_is_rejected_loudly() {
        let host = StubHost {
            session: None,
            exists: true,
        };
        let registry = shared();
        let err = handle_watch(
            &host,
            &registry,
            &Translator::default(),
            json!({ "surface": 7 }),
        )
        .expect_err("must reject");
        // Translator::default() 는 키를 그대로 돌려주므로 키가 보이면 그 분기다.
        assert!(err.message.contains("no_session_meta"), "{}", err.message);
        assert!(
            !registry.lock().expect("lock").is_watched(7),
            "a rejected watch must not leave a registration behind"
        );
    }

    #[test]
    fn unwatch_of_an_unknown_surface_is_an_error_not_a_silent_ok() {
        let err = handle_unwatch(
            &live(),
            &shared(),
            &Translator::default(),
            json!({ "surface": 3 }),
        )
        .expect_err("must reject");
        assert!(err.message.contains("not_watched"), "{}", err.message);
    }

    #[test]
    fn poll_clamps_the_limit_and_defaults_the_cursor() {
        let registry = shared();
        {
            let mut reg = registry.lock().expect("lock");
            for _ in 0..5 {
                reg.push_event(1, "s", crate::record::StreamEvent::turn_end("end_turn"));
            }
        }
        let all = handle_poll(&registry, &Translator::default(), json!({})).expect("poll");
        assert_eq!(all["events"].as_array().expect("array").len(), 5);

        let capped = handle_poll(&registry, &Translator::default(), json!({ "limit": 99999 }))
            .expect("poll");
        assert_eq!(capped["events"].as_array().expect("array").len(), 5);

        let one = handle_poll(
            &registry,
            &Translator::default(),
            json!({ "limit": 1, "after_seq": 2 }),
        )
        .expect("poll");
        let events = one["events"].as_array().expect("array");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["seq"], 3);
    }

    #[test]
    fn poll_filter_surface_is_not_auto_filled_from_the_surface_key() {
        let registry = shared();
        {
            let mut reg = registry.lock().expect("lock");
            reg.push_event(1, "s", crate::record::StreamEvent::turn_end("end_turn"));
            reg.push_event(2, "s", crate::record::StreamEvent::turn_end("end_turn"));
        }
        // CLI 가 TASTY_SURFACE_ID 로 자동 주입하는 `surface`/`surface_id` 키는 poll 의
        // 필터로 쓰이지 않는다 — 필터는 오직 `filter_surface` 다.
        let unfiltered = handle_poll(
            &registry,
            &Translator::default(),
            json!({ "surface": 1, "surface_id": 1 }),
        )
        .expect("poll");
        assert_eq!(unfiltered["events"].as_array().expect("array").len(), 2);

        let filtered = handle_poll(
            &registry,
            &Translator::default(),
            json!({ "filter_surface": 2 }),
        )
        .expect("poll");
        assert_eq!(filtered["events"].as_array().expect("array").len(), 1);
    }

    #[test]
    fn list_is_empty_before_anything_is_watched() {
        let list = handle_list(&shared(), &Translator::default()).expect("list");
        assert!(list["watches"].as_array().expect("array").is_empty());
    }

    fn watched_shared(surface_id: u32) -> Shared {
        let registry = shared();
        registry.lock().expect("lock").insert(new_watch(
            surface_id,
            "s".into(),
            std::path::PathBuf::new(),
            false,
        ));
        registry
    }

    #[test]
    fn turn_start_opens_a_turn_on_a_watched_surface() {
        let registry = watched_shared(3);
        let out = handle_turn_start(
            &live(),
            &registry,
            &Translator::default(),
            json!({ "surface": 3, "request_id": "abc" }),
        )
        .expect("opens");
        assert_eq!(out["surface_id"], 3);
        assert_eq!(out["request_id"], "abc");
        assert_eq!(out["timeout_secs"], DEFAULT_TURN_TIMEOUT_SECS);
        assert!(registry.lock().expect("lock").has_open_turn(3));
    }

    #[test]
    fn turn_start_accepts_a_numeric_request_id_as_a_string() {
        // `${body.request_id}` 는 전체 플레이스홀더면 타입을 보존한다 — 숫자로 와도 받는다.
        let registry = watched_shared(3);
        let out = handle_turn_start(
            &live(),
            &registry,
            &Translator::default(),
            json!({ "surface": 3, "request_id": 42 }),
        )
        .expect("opens");
        assert_eq!(out["request_id"], "42");
    }

    #[test]
    fn turn_start_without_a_request_id_is_rejected() {
        let registry = watched_shared(3);
        let err = handle_turn_start(
            &live(),
            &registry,
            &Translator::default(),
            json!({ "surface": 3 }),
        )
        .expect_err("must reject");
        assert_eq!(err.code, -32602);
        assert!(
            err.message.contains("missing_request_id"),
            "{}",
            err.message
        );
        assert!(!registry.lock().expect("lock").has_open_turn(3));
    }

    #[test]
    fn turn_start_rejects_an_oversized_request_id() {
        let registry = watched_shared(3);
        let huge = "x".repeat(MAX_REQUEST_ID_LEN + 1);
        let err = handle_turn_start(
            &live(),
            &registry,
            &Translator::default(),
            json!({ "surface": 3, "request_id": huge }),
        )
        .expect_err("must reject an oversized request_id");
        assert_eq!(err.code, -32602);
        assert!(
            err.message.contains("request_id_too_long"),
            "{}",
            err.message
        );
        // 거부된 요청은 어떤 턴도 열지 않는다 — 증폭 벡터가 저장 단계에 닿지 않는다.
        assert!(!registry.lock().expect("lock").has_open_turn(3));
    }

    #[test]
    fn turn_start_accepts_a_request_id_at_the_cap() {
        let registry = watched_shared(3);
        let at_cap = "x".repeat(MAX_REQUEST_ID_LEN);
        handle_turn_start(
            &live(),
            &registry,
            &Translator::default(),
            json!({ "surface": 3, "request_id": at_cap }),
        )
        .expect("a request_id exactly at the cap is accepted");
        assert!(registry.lock().expect("lock").has_open_turn(3));
    }

    /// 없는 surface 를 **호스트가 실제로 답하는 방식**으로 답하는 host.
    ///
    /// `{exists:false}` 가 아니라 요청 거절이다 — 실측(2026-09-05, 격리 헤드리스
    /// 인스턴스). 스텁이 `{exists:false}` 를 쓰면 이 테스트는 프로덕션에서 한 번도
    /// 돌지 않는 경로를 재게 된다(그 함정에 `pump` 의 기존 회귀가 걸려 있었다).
    struct RejectingHost;

    impl HostCall for RejectingHost {
        fn call(&self, method: &str, params: Value) -> Result<Value, PluginError> {
            let sid = params
                .get("surface_id")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            match method {
                // 없는 대상에는 **어느 호스트 호출이든** 같은 모양으로 거절한다 — 실측이
                // 그렇다. 그래서 스텁도 메서드마다 다른 답을 흉내 내지 않는다.
                "surface.locate" | "surface.meta.get" => Err(PluginError::HostCall {
                    method: method.to_string(),
                    message: tasty_utils::target::unowned_target_message("surface", sid, method),
                    code: None,
                }),
                other => panic!("unexpected host call {other}"),
            }
        }
    }

    /// `watch` 도 **호출자가 부른 메서드**를 괄호에 넣는다 — 내부 호출 이름이 아니다.
    ///
    /// 실측(2026-09-05): 없는 id 로 `agent_stream.watch` 를 부르면 호스트의 거절이 그대로
    /// 실려 나와 `(named by 'surface.meta.get')` 이 됐다. 호출자는 그 이름을 지목한 적이
    /// 없고, 같은 사정에서 `turn_start`·`unwatch` 는 자기 이름을 답한다. 한 사정에 세 가지
    /// 이름이 나오면 그 괄호는 읽을 수 없는 칸이 된다.
    #[test]
    fn watch_names_the_method_the_caller_called() {
        let tr = Translator::default();
        let err = handle_watch(
            &RejectingHost,
            &shared(),
            &tr,
            json!({ "surface": 424_242 }),
        )
        .expect_err("없는 surface 는 거절");
        assert_eq!(
            err.message,
            tasty_utils::target::unowned_target_message("surface", 424_242, "agent_stream.watch"),
            "내부 호출 이름이 새어 나왔다: {}",
            err.message
        );
    }

    /// 값을 실어 보냈는데 그 값이 surface id 가 될 수 없으면 **"안 줬다" 가 아니다.**
    ///
    /// 실측(2026-09-05, 격리 헤드리스 인스턴스): 세 메서드 전부 `surface: 999999999999`
    /// 에 `missing_surface` 로 답했다 — 키를 아예 뺀 요청과 **글자 하나 다르지 않았다.**
    #[test]
    fn a_surface_value_out_of_u32_range_is_not_reported_as_missing() {
        let tr = Translator::default();
        let absent = require_surface(&json!({}), &tr).expect_err("키 없음은 거절");
        let too_big = require_surface(&json!({ "surface": 999_999_999_999u64 }), &tr)
            .expect_err("범위 초과는 거절");
        assert!(
            absent.message.contains("missing_surface"),
            "키가 없으면 missing: {}",
            absent.message
        );
        assert!(
            too_big.message.contains("invalid_surface"),
            "값이 있으면 invalid: {}",
            too_big.message
        );
        assert_ne!(
            absent.message, too_big.message,
            "두 사정이 같은 문장이면 호출자는 같은 값을 다시 보낸다"
        );
        // 양방향 — 정상 값은 그대로 통과한다.
        assert_eq!(
            require_surface(&json!({ "surface": 7 }), &tr).expect("정상"),
            7
        );
    }

    /// **없는 surface** 에 "안 보고 있다" 고 답하지 않는다 — 두 메서드 모두.
    ///
    /// 실측(같은 회차): 존재하지 않는 424242 와 **살아 있는** 1 이 `turn_start`·`unwatch`
    /// 에서 같은 문장을 받았다. 호출자는 `watch` 를 부르면 되는 줄 알고 다시 부른다.
    #[test]
    fn a_missing_surface_is_not_reported_as_merely_unwatched() {
        let tr = Translator::default();

        let dead_turn = handle_turn_start(
            &RejectingHost,
            &shared(),
            &tr,
            json!({ "surface": 424_242, "request_id": "abc" }),
        )
        .expect_err("없는 surface 는 거절");
        let dead_unwatch = handle_unwatch(
            &RejectingHost,
            &shared(),
            &tr,
            json!({ "surface": 424_242 }),
        )
        .expect_err("없는 surface 는 거절");
        // 단언 대상이 **정본 함수의 출력 그대로**다. 예전에는 `contains("no_live_surface")`
        // 로 **번역 키 이름**을 봤는데, 그건 키가 lang 파일에 없을 때 키를 그대로 돌려주는
        // 스텁 Translator 덕에 통과하던 것이라 문장에 대해 아무것도 안 재고 있었다.
        // 여기서 재는 것은 포맷이 아니라 **`method` 자리에 무엇이 들어가는가** 다 —
        // 틀렸던 것이 정확히 그 자리(인자 이름 `'surface'`)였다.
        assert_eq!(
            dead_turn.message,
            tasty_utils::target::unowned_target_message(
                "surface",
                424_242,
                "agent_stream.turn_start"
            )
        );
        assert_eq!(
            dead_unwatch.message,
            tasty_utils::target::unowned_target_message("surface", 424_242, "agent_stream.unwatch")
        );

        // **양방향** — 살아 있지만 watch 안 한 surface 는 예전 문장 그대로여야 한다.
        // 이쪽이 무너지면 위 둘은 "전부 없는 surface 라고 답한다" 는 뜻이 된다.
        let live_turn = handle_turn_start(
            &live(),
            &shared(),
            &tr,
            json!({ "surface": 3, "request_id": "abc" }),
        )
        .expect_err("watch 안 했으면 거절");
        let live_unwatch = handle_unwatch(&live(), &shared(), &tr, json!({ "surface": 3 }))
            .expect_err("watch 안 했으면 거절");
        assert!(
            live_turn.message.contains("turn_not_watched"),
            "{}",
            live_turn.message
        );
        assert!(
            live_unwatch.message.contains("not_watched"),
            "{}",
            live_unwatch.message
        );
    }

    #[test]
    fn turn_start_on_an_unwatched_surface_is_rejected_loudly() {
        let err = handle_turn_start(
            &live(),
            &shared(),
            &Translator::default(),
            json!({ "surface": 3, "request_id": "abc" }),
        )
        .expect_err("must reject");
        assert!(err.message.contains("turn_not_watched"), "{}", err.message);
    }

    #[test]
    fn turn_start_rejects_an_overlapping_turn() {
        let registry = watched_shared(3);
        handle_turn_start(
            &live(),
            &registry,
            &Translator::default(),
            json!({ "surface": 3, "request_id": "first" }),
        )
        .expect("first opens");
        let err = handle_turn_start(
            &live(),
            &registry,
            &Translator::default(),
            json!({ "surface": 3, "request_id": "second" }),
        )
        .expect_err("overlap rejected");
        assert!(err.message.contains("turn_already_open"), "{}", err.message);
    }

    #[test]
    fn turn_start_clamps_the_timeout_into_range() {
        let registry = watched_shared(3);
        let out = handle_turn_start(
            &live(),
            &registry,
            &Translator::default(),
            json!({ "surface": 3, "request_id": "abc", "timeout_secs": 1 }),
        )
        .expect("opens");
        assert_eq!(out["timeout_secs"], MIN_TURN_TIMEOUT_SECS);
    }
}
