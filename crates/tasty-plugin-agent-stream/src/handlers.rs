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

use serde_json::{Value, json};
use tasty_plugin_sdk::{IpcMethodError, i18n::Translator};

use crate::registry::{StreamRegistry, new_watch};
use crate::resolve::{self, CLAUDE_SESSION_META_KEY, HostCall, ResolveError};

/// `poll` 의 기본/최대 반환 개수.
const POLL_DEFAULT_LIMIT: u64 = 100;
const POLL_MAX_LIMIT: u64 = 1000;

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

fn require_surface(params: &Value, tr: &Translator) -> Result<u32, IpcMethodError> {
    params
        .get("surface")
        .or_else(|| params.get("surface_id"))
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("agent_stream.error.missing_surface")))
}

fn resolve_error_message(tr: &Translator, err: &ResolveError) -> IpcMethodError {
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
        ResolveError::HostCall { message } => IpcMethodError::new(message.clone()),
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
        .map_err(|e| resolve_error_message(tr, &e))?;

    let transcript = match resolve::transcript_path(&session_id) {
        Ok(path) => Some(path),
        Err(ResolveError::TranscriptNotFound { .. }) => None,
        Err(e) => return Err(resolve_error_message(tr, &e)),
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

/// `agent_stream.unwatch` — tail 을 멈추고 종료 이벤트를 남긴다.
pub fn handle_unwatch(
    registry: &Shared,
    tr: &Translator,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface(&params, tr)?;
    let mut reg = lock(registry, tr)?;
    if !reg.remove(surface_id, crate::record::REASON_UNWATCHED) {
        return Err(IpcMethodError::new(tr.t_replace(
            "agent_stream.error.not_watched",
            "{surface}",
            &surface_id.to_string(),
        )));
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

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_plugin_sdk::PluginError;

    struct StubHost {
        session: Option<&'static str>,
    }

    impl HostCall for StubHost {
        fn call(&self, method: &str, _params: Value) -> Result<Value, PluginError> {
            match method {
                "surface.meta.get" => Ok(json!({ "value": self.session })),
                "surface.locate" => Ok(json!({ "exists": true })),
                other => panic!("unexpected host call {other}"),
            }
        }
    }

    fn shared() -> Shared {
        Arc::new(Mutex::new(StreamRegistry::new(None)))
    }

    #[test]
    fn watch_without_a_target_surface_is_rejected() {
        let host = StubHost { session: Some("s") };
        let err = handle_watch(&host, &shared(), &Translator::default(), json!({}))
            .expect_err("must reject");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("missing_surface") || err.message.contains("--surface"));
    }

    #[test]
    fn watch_without_session_meta_is_rejected_loudly() {
        let host = StubHost { session: None };
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
        let err = handle_unwatch(&shared(), &Translator::default(), json!({ "surface": 3 }))
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
}
