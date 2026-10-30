//! attach mirror 의 원격 자식 Codex.
//!
//! - client(부모 쪽): 부모·Codex 수신처·완료 알림 훅은 이 Tasty 의 mirror surface 에 묶고, 자식 생성·입력·종료만
//!   서버의 같은 메서드(`codex.spawn`·`tell`·`kill`)로 보낸다. 서버로 보내는 인자는 허용 목록으로 고르며 이 Tasty 의
//!   호출 문맥·수신처·부모는 보내지 않는다.
//! - 서버(자식 쪽): host 가 넣은 원격 출처(`attach:o<n>` + 문맥 토큰)로 받은 호출이다. 서버의 Codex CLI·설정·
//!   승인 정책으로 실행하고 이 부모에게는 알림 훅을 걸지 않는다. 부모가 Codex 대화면 확정한 턴을
//!   [`crate::delivery::attach_out`] 으로 돌려보낸다.
//! - `codex.remote_receive` 는 결과를 내부 [`crate::delivery::deliver`] 로 직접 기록한다. 같은 worker 에서 자기
//!   namespace(`codex.deliver`)를 host 로 다시 부르면 교착된다.
//!
//! 계약: docs/plugins/remote-attach-children.md.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use tasty_plugin_agent_common::remote_attach::{
    self, AttachOrigin, CallError, Parent, RETURN_TURNS_KEY, Received, Request, Target, TargetError,
};

use super::*;
use crate::delivery::ledger;

/// 서버 `codex.spawn` 으로 보내는 인자. 승인 정책 기본값은 서버의 plugin 설정으로 정한다.
const SPAWN_KEYS: &[&str] = &[
    "cwd",
    "prompt",
    "model",
    "effort",
    "approval",
    "sandbox",
    "full_auto",
];

/// 결과불명으로 끝난 spawn 의 부모. host 가 늦은 결과(`late_spawn`)를 최대 10분 다시 주므로 조금 길게 둔다.
struct PendingSpawn {
    receiver_id: Option<String>,
    parent_surface: Option<u32>,
    at: Instant,
}

const PENDING_KEPT: Duration = Duration::from_secs(15 * 60);

static PENDING: LazyLock<Mutex<HashMap<u64, PendingSpawn>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn pending() -> std::sync::MutexGuard<'static, HashMap<u64, PendingSpawn>> {
    PENDING.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("codex remote: pending spawn table was poisoned; continuing with its data");
        poisoned.into_inner()
    })
}

/// host 의 결과불명 메시지에 실린 call 번호(`call <id>`).
fn call_id_in(message: &str) -> Option<u64> {
    let rest = &message[message.find("call ")? + "call ".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn target_error(e: TargetError, tr: &Translator) -> IpcMethodError {
    match e {
        TargetError::Unsupported => IpcMethodError::new(tr.t("codex.remote.unsupported")),
        TargetError::Host(m) => {
            IpcMethodError::new(tr.t_replace("codex.remote.lookup_failed", "{detail}", &m))
        }
    }
}

fn call_error(e: CallError, tr: &Translator) -> IpcMethodError {
    match e {
        CallError::Unknown(m) => {
            IpcMethodError::new(tr.t_replace("codex.remote.outcome_unknown", "{detail}", &m))
        }
        CallError::Failed { code, message } => {
            let mut error = IpcMethodError::new(message);
            if let Some(code) = code {
                error.code = code;
            }
            error
        }
    }
}

/// spawn 대상(workspace 숫자 id·pane)이 attach mirror 면 실행 위치 surface 를 준다.
pub(crate) fn spawn_anchor<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<Option<u32>, IpcMethodError> {
    let mut query = Map::new();
    if let Some(ws) = params
        .get("workspace")
        .and_then(|w| w.as_u64().or_else(|| w.as_str()?.parse().ok()))
    {
        query.insert("workspace".into(), json!(ws));
    }
    if let Some(pane) = params.get("pane").and_then(Value::as_u64) {
        query.insert("pane".into(), json!(pane));
    }
    if query.is_empty() {
        return Ok(None);
    }
    match remote_attach::target(host, query).map_err(|e| target_error(e, tr))? {
        Target::Local => Ok(None),
        Target::Remote {
            anchor_surface_id, ..
        } => Ok(Some(anchor_surface_id)),
    }
}

/// 이 surface 가 이 Tasty 에 등록된 원격 자식이면 실행 위치 surface 를 준다.
pub(crate) fn child_anchor<H: HostCall>(
    host: &H,
    surface: u32,
    tr: &Translator,
) -> Result<Option<u32>, IpcMethodError> {
    let query = Map::from_iter([("surface_id".to_string(), json!(surface))]);
    match remote_attach::target(host, query).map_err(|e| target_error(e, tr))? {
        Target::Remote {
            anchor_surface_id,
            child: true,
        } => Ok(Some(anchor_surface_id)),
        _ => Ok(None),
    }
}

fn child_request(
    method: &'static str,
    anchor: u32,
    child: u32,
    params: Map<String, Value>,
) -> Request {
    Request {
        method,
        anchor_surface_id: anchor,
        target_surface_id: Some(child),
        params,
        parent: None,
        role: None,
        nickname: None,
    }
}

/// `terminal.spawn` 부모 인자를 원격 등록 부모로 바꾼다.
fn parent_of(parent: &Map<String, Value>) -> Option<Parent> {
    let surface = |key: &str| {
        parent
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    if let Some(key) = parent.get("parent_external").and_then(Value::as_str) {
        return Some(Parent::External(key.to_string()));
    }
    surface("parent")
        .or_else(|| surface("surface"))
        .map(Parent::Surface)
}

/// client: 새 mirror 자식에 부모 경로를 묶는다. Codex 부모는 결과 수신처, 물리 부모는 완료 알림 훅이다.
fn bind<H: HostCall>(
    host: &H,
    delivery: &DeliveryCtx<'_>,
    child: u32,
    index: Option<u32>,
    receiver_id: Option<&str>,
    parent_surface: Option<u32>,
) -> Result<(), IpcMethodError> {
    match receiver_id {
        Some(id) => ledger::set_child_receiver(delivery.root, child, id, index)
            .map_err(|e| IpcMethodError::new(format!("codex child return record: {e}"))),
        None => {
            ownership::invalidate_route(delivery, child)?;
            if let Some(parent) = parent_surface {
                register_notify_hooks(host, parent, child, "spawn");
            }
            Ok(())
        }
    }
}

/// client: mirror workspace 에 자식을 만든다. 서버에서 실행한 뒤 mirror 자식에 경로를 묶고, 묶기에 성공한 뒤에만
/// 통지를 풀어 준다(commit). 묶지 못하면 commit 하지 않고 그 자식을 닫는다.
pub(crate) fn spawn<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
    delivery: &DeliveryCtx<'_>,
    anchor: u32,
    receiver_id: Option<&str>,
    parent: &Map<String, Value>,
) -> Result<Value, IpcMethodError> {
    let mut remote = forward(params, SPAWN_KEYS);
    remote.insert(RETURN_TURNS_KEY.into(), json!(receiver_id.is_some()));
    let parent_surface = match receiver_id {
        Some(_) => None,
        None => parent
            .get("surface")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
    };
    let reply = match remote_attach::call(
        host,
        Request {
            method: "spawn",
            anchor_surface_id: anchor,
            target_surface_id: None,
            params: remote,
            parent: parent_of(parent),
            role: params.get("role").cloned(),
            nickname: params.get("nickname").cloned(),
        },
    ) {
        Ok(reply) => reply,
        Err(e) => {
            if let CallError::Unknown(message) = &e
                && let Some(call_id) = call_id_in(message)
            {
                let mut table = pending();
                table.retain(|_, p| p.at.elapsed() < PENDING_KEPT);
                table.insert(
                    call_id,
                    PendingSpawn {
                        receiver_id: receiver_id.map(str::to_string),
                        parent_surface,
                        at: Instant::now(),
                    },
                );
            }
            return Err(call_error(e, tr));
        }
    };
    let Some(child) = reply.children.first().cloned() else {
        if let Err(e) = remote_attach::commit(host, reply.call_id) {
            tracing::warn!(
                "codex remote spawn: commit of call {} failed: {e}",
                reply.call_id
            );
        }
        return Err(IpcMethodError::new(tr.t_replace(
            "codex.remote.no_child",
            "{resp}",
            &reply.result.to_string(),
        )));
    };
    if let Err(e) = bind(
        host,
        delivery,
        child.surface_id,
        child.index,
        receiver_id,
        parent_surface,
    ) {
        let cleanup = match remote_attach::call(
            host,
            child_request("kill", anchor, child.surface_id, Map::new()),
        ) {
            Ok(_) => tr.t("codex.remote.child_closed").to_string(),
            Err(k) => tr.t_replace("codex.remote.child_not_closed", "{detail}", &k.to_string()),
        };
        return Err(IpcMethodError::new(t_args(
            tr,
            "codex.remote.bind_failed",
            &[
                ("{surface}", &child.surface_id.to_string()),
                ("{detail}", &e.message),
                ("{cleanup}", &cleanup),
            ],
        )));
    }
    let committed = remote_attach::commit(host, reply.call_id);
    let mut out = Map::new();
    out.insert("child_surface_id".into(), json!(child.surface_id));
    if let Some(index) = child.index {
        out.insert("child_index".into(), json!(index));
    }
    out.insert("remote".into(), json!(true));
    out.insert(
        "remote_child_surface_id".into(),
        json!(child.remote_surface_id),
    );
    match parent_of(parent) {
        Some(Parent::Surface(id)) => out.insert("parent_surface_id".into(), json!(id)),
        Some(Parent::External(key)) => out.insert("parent_external".into(), json!(key)),
        None => None,
    };
    if let Some(w) = reply.result.get("warning").filter(|v| !v.is_null()) {
        out.insert("warning".into(), w.clone());
    }
    if let Err(e) = committed {
        out.insert(
            "warning".into(),
            json!(tr.t_replace("codex.remote.commit_failed", "{detail}", &e)),
        );
    }
    Ok(Value::Object(out))
}

/// client: 원격 자식에게 입력을 보낸다. 결과 경로는 호출자가 이미 정했다.
pub(crate) fn tell<H: HostCall>(
    host: &H,
    anchor: u32,
    child: u32,
    message: &str,
    codex: bool,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let params = Map::from_iter([
        ("message".to_string(), json!(message)),
        (RETURN_TURNS_KEY.to_string(), json!(codex)),
    ]);
    let reply = remote_attach::call(host, child_request("tell", anchor, child, params))
        .map_err(|e| call_error(e, tr))?;
    let mut out = match reply.result {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    out.insert("surface".into(), json!(child));
    out.insert("remote".into(), json!(true));
    Ok(Value::Object(out))
}

/// 이 부모 인자 아래 index 의 자식 surface.
pub(crate) fn child_by_index<H: HostCall>(
    host: &H,
    parent: &Map<String, Value>,
    index: u32,
) -> Option<u32> {
    let mut p = parent.clone();
    p.remove("child");
    let resp = host.call("terminal.children", Value::Object(p)).ok()?;
    resp.get("children")?.as_array()?.iter().find_map(|c| {
        (c.get("index").and_then(Value::as_u64) == Some(u64::from(index)))
            .then(|| c.get("surface_id").and_then(Value::as_u64))
            .flatten()
            .and_then(|n| u32::try_from(n).ok())
    })
}

/// client: 원격 자식을 닫는다. 결과 기록은 끝난 기록으로 남겨 서버에 남은 결과가 늦게 와도 기록한다.
pub(crate) fn kill<H: HostCall>(
    host: &H,
    delivery: &DeliveryCtx<'_>,
    anchor: u32,
    child: u32,
    index: u32,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let reply = remote_attach::call(host, child_request("kill", anchor, child, Map::new()))
        .map_err(|e| call_error(e, tr))?;
    let killed = reply.result.get("killed_surface_id").is_some()
        || reply.result.get("killed").and_then(Value::as_bool) == Some(true);
    if killed && let Err(e) = ledger::end_child_receiver(delivery.root, child) {
        tracing::warn!("codex child return record for remote child {child}: {e}");
    }
    let mut out = Map::new();
    if killed {
        out.insert("killed_surface_id".into(), json!(child));
    }
    out.insert("child_index".into(), json!(index));
    out.insert("remote".into(), json!(true));
    Ok(Value::Object(out))
}

/// client: `codex.remote_receive`. 원격 결과는 mirror 자식 경로의 수신처 원장에 직접 기록한다.
pub(crate) fn receive<H: HostCall>(
    host: &H,
    delivery: &DeliveryCtx<'_>,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    if params.get("kind").and_then(Value::as_str) == Some("late_spawn") {
        return Ok(late_spawn(host, delivery, params, tr).json());
    }
    let Some((surface, payload)) = remote_attach::received(params) else {
        return Ok(Received::Rejected("missing surface_id or payload".into()).json());
    };
    let record = ledger::child_receiver(delivery.root, surface)
        .map_err(|e| IpcMethodError::new(format!("codex child return record: {e}")))?;
    let Some(receiver_id) = record.map(|r| r.receiver_id).filter(|r| !r.is_empty()) else {
        return Ok(Received::Rejected(format!("no Codex return route for s{surface}")).json());
    };
    let Some(receiver) = ledger::read_receiver(delivery.root, &receiver_id)
        .map_err(|e| IpcMethodError::new(format!("codex receiver: {e}")))?
    else {
        return Ok(Received::Rejected(format!("receiver of s{surface} is gone")).json());
    };
    let mut completion = match payload
        .get(crate::delivery::attach_out::PAYLOAD_KEY)
        .ok_or_else(|| "payload without a completion".to_string())
        .and_then(crate::delivery::parse_completion)
    {
        Ok(c) => c,
        Err(reason) => return Ok(Received::Rejected(reason).json()),
    };
    completion.completion_key = remote_attach::local_completion_key(&completion.completion_key);
    completion.child_surface = surface;
    let (_, duplicate) = crate::delivery::deliver(delivery, &receiver, completion)
        .map_err(|e| IpcMethodError::new(format!("codex ledger: {e}")))?;
    Ok(if duplicate {
        Received::Duplicate
    } else {
        Received::Recorded
    }
    .json())
}

fn late_spawn<H: HostCall>(
    host: &H,
    delivery: &DeliveryCtx<'_>,
    params: &Value,
    tr: &Translator,
) -> Received {
    let children: Vec<(u32, Option<u32>)> = params
        .get("children")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let s = u32::try_from(c.get("surface_id")?.as_u64()?).ok()?;
                    let i = c
                        .get("index")
                        .and_then(Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok());
                    Some((s, i))
                })
                .collect()
        })
        .unwrap_or_default();
    let waiting = params
        .get("call_id")
        .and_then(Value::as_u64)
        .and_then(|id| pending().remove(&id));
    let failed = params.get("error").is_some_and(|e| !e.is_null());
    let reason = match (&waiting, failed, children.as_slice()) {
        (_, _, []) => "the late result has no child".to_string(),
        (None, _, _) => "no parent is waiting for this call (plugin restarted or expired)".into(),
        (Some(_), true, _) => "the server reported an error for this call".into(),
        (Some(p), false, [(child, index)]) => match bind(
            host,
            delivery,
            *child,
            *index,
            p.receiver_id.as_deref(),
            p.parent_surface,
        ) {
            Ok(()) => return Received::Recorded,
            Err(e) => e.message,
        },
        (Some(_), false, _) => "a spawn call cannot create several children".into(),
    };
    for (child, _) in &children {
        let closed = child_anchor(host, *child, tr).and_then(|anchor| match anchor {
            Some(anchor) => {
                remote_attach::call(host, child_request("kill", anchor, *child, Map::new()))
                    .map(|_| ())
                    .map_err(|e| call_error(e, tr))
            }
            None => Ok(()),
        });
        if let Err(e) = closed {
            tracing::warn!(
                "codex remote: late child s{child} could not be closed: {}",
                e.message
            );
        }
    }
    tracing::warn!("codex remote: late spawn refused: {reason}");
    Received::Rejected(reason)
}

/// 서버: 원격 출처 부모의 새 자식을 이 Tasty 의 Codex CLI·설정으로 실행한다.
pub(crate) fn serve_spawn<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
    delivery: &DeliveryCtx<'_>,
    origin: &AttachOrigin,
) -> Result<Value, IpcMethodError> {
    let mut args = crate::spawn_options::parse(params, tr)?;
    let policy = resolve_policy_args(host, params, tr)?;
    let mut policy = crate::session::command::policy_tokens(&policy);
    policy.append(&mut args);
    let prompt = optional_str(params, "prompt");
    let mut sp = forward(params, &["cwd", "role", "nickname"]);
    AttachOrigin::spawn_request(params, &mut sp);
    let resp = host_call(host, "terminal.spawn", Value::Object(sp))?;
    let child = resp
        .get("child_surface_id")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| {
            IpcMethodError::new(tr.t_replace(
                "codex.spawn.missing_child_surface_id",
                "{resp}",
                &resp.to_string(),
            ))
        })?;
    if origin.return_turns {
        ledger::set_attach_child(
            delivery.root,
            child,
            &origin.parent_key,
            child_index_of(&resp),
        )
        .map_err(|e| IpcMethodError::new(format!("codex child return record: {e}")))?;
    } else {
        ownership::invalidate_route(delivery, child)?;
    }
    let cmd = make_codex_command_with_args(child, prompt.as_deref(), &policy)?;
    host_call(
        host,
        "surface.send",
        json!({ "surface_id": child, "text": cmd }),
    )?;
    let mut out = resp;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("parent_external".into(), json!(origin.parent_key));
    }
    Ok(out)
}

fn origin_target<H: HostCall>(
    host: &H,
    origin: &AttachOrigin,
    tr: &Translator,
) -> Result<u32, IpcMethodError> {
    remote_attach::origin_child(host, origin)
        .map_err(IpcMethodError::from)?
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.remote.no_target")))
}

/// 서버: 원격 출처 부모가 자기 자식에게 보낸 입력. 결과 경로를 먼저 정하고, 보내지 못하면 되돌린다.
pub(crate) fn serve_tell<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
    delivery: &DeliveryCtx<'_>,
    origin: &AttachOrigin,
) -> Result<Value, IpcMethodError> {
    let child = origin_target(host, origin, tr)?;
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.params.missing_message")))?;
    let previous = ledger::child_receiver(delivery.root, child)
        .map_err(|e| IpcMethodError::new(format!("codex child return record: {e}")))?;
    if origin.return_turns {
        let index = previous.as_ref().and_then(|p| p.child_index);
        ledger::set_attach_child(delivery.root, child, &origin.parent_key, index)
            .map_err(|e| IpcMethodError::new(format!("codex child return record: {e}")))?;
    } else {
        ownership::invalidate_route(delivery, child)?;
    }
    match host_call(
        host,
        "terminal.tell",
        json!({ "surface": child, "text": message }),
    ) {
        Ok(resp) => Ok(resp),
        Err(e) => {
            if origin.return_turns {
                ownership::restore_route(delivery, child, previous);
            }
            Err(e)
        }
    }
}

/// 서버: 원격 출처 부모의 자식을 닫는다. 이미 확정해 보낼 기록으로 옮긴 결과는 계속 전달된다.
pub(crate) fn serve_kill<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
    delivery: &DeliveryCtx<'_>,
    origin: &AttachOrigin,
) -> Result<Value, IpcMethodError> {
    let child = origin_target(host, origin, tr)?;
    let mut parent = forward(params, &["workspace"]);
    parent.insert("parent_external".into(), json!(origin.parent_key));
    let children = host_call(host, "terminal.children", Value::Object(parent.clone()))?;
    let index = children
        .get("children")
        .and_then(Value::as_array)
        .and_then(|a| {
            a.iter().find_map(|c| {
                (c.get("surface_id").and_then(Value::as_u64) == Some(u64::from(child)))
                    .then(|| c.get("index").and_then(Value::as_u64))
                    .flatten()
            })
        })
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.remote.no_target")))?;
    parent.insert("child".into(), json!(index));
    let resp = host_call(host, "terminal.kill", Value::Object(parent))?;
    if let Some(killed) = resp
        .get("killed_surface_id")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        && let Err(e) = ledger::clear_child_receiver(delivery.root, killed)
    {
        tracing::warn!("codex child return record for killed surface {killed}: {e}");
    }
    Ok(resp)
}

/// 서버의 공개 호출이 attach 원격 자식을 바꾸려 하면 거절한다([`remote_attach::refuse_public`]).
pub(crate) fn refuse_public<H: HostCall>(
    host: &H,
    method: &str,
    params: &Value,
    tr: &Translator,
) -> Result<(), IpcMethodError> {
    match remote_attach::refuse_public(host, method, params) {
        Ok(None) => Ok(()),
        Ok(Some(remote_attach::Refusal::Child(surface))) => {
            Err(IpcMethodError::invalid_params(&tr.t_replace(
                "codex.remote.attach_child",
                "{surface}",
                &surface.to_string(),
            )))
        }
        Ok(Some(remote_attach::Refusal::Parent)) => Err(IpcMethodError::invalid_params(
            tr.t("codex.remote.attach_parent"),
        )),
        Err(e) => Err(IpcMethodError::new(tr.t_replace(
            "codex.remote.lookup_failed",
            "{detail}",
            &e.to_string(),
        ))),
    }
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
