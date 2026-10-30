//! attach 원격 출처 부모의 턴 결과. 이 Tasty 는 부모의 수신처를 모르므로 결과를 서버 host 에 맡기고
//! (`attach.child_notice`), client 의 같은 plugin 이 그 부모의 수신처로 기록한다([`super::Returns::remote_receive`]).
//!
//! 영속 요청은 client 가 기록을 확인한(`delivered`) 뒤나 받을 수 없다고 한(`rejected`) 뒤에만 지운다.
//! `pending` 은 송신 실패와 같이 같은 키로 늘어나는 간격을 두고 다시 묻는다.

use std::time::Duration;

use serde_json::Value;
use tasty_plugin_agent_common::host_call::HostCall;
use tasty_plugin_agent_common::remote_attach::{self, NoticeStatus};
use tasty_plugin_sdk::IpcMethodError;

use super::{DELIVER_CALL_TIMEOUT, Returns, ms, now_ms};

/// client 가 받을 결과 본문의 키. 나머지 형식은 이 plugin 이 정하며 host 는 해석하지 않는다.
pub(crate) const PAYLOAD_KEY: &str = "completion";

/// attach 원격 부모로 갈 요청인가.
pub(super) fn is_attach(request: &Value) -> bool {
    request.get("attach").and_then(Value::as_bool) == Some(true)
}

/// 보내지 못한 요청의 처리.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Failure {
    /// 일시 실패나 아직 확인 전이다. 늘어나는 간격으로 다시 보낸다(기한이 지나면 포기한다).
    Retry(String),
    /// 받는 쪽이 영구히 받을 수 없다고 했다. 다시 보내지 않고 사유와 함께 포기한 요청으로 남긴다.
    Refused(String),
}

/// 요청을 서버 host 에 맡기거나 상태를 묻는다. client 가 기록했으면 `Ok` 이고 그때만 지운다.
pub(super) fn hand_over<H: HostCall>(host: &H, mut request: Value) -> Result<Value, Failure> {
    let surface = request
        .get("child_surface")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| Failure::Refused("attach result without child_surface".into()))?;
    let key = super::key_of(&request)
        .ok_or_else(|| Failure::Refused("attach result without completion_key".into()))?
        .to_string();
    if let Some(map) = request.as_object_mut() {
        map.remove("attach");
    }
    let payload = serde_json::json!({ PAYLOAD_KEY: request });
    match remote_attach::child_notice(host, surface, &key, payload).map_err(Failure::Retry)? {
        NoticeStatus::Delivered => Ok(serde_json::json!({ "attach": "delivered" })),
        NoticeStatus::Rejected(reason) => Err(Failure::Refused(reason)),
        NoticeStatus::Pending => Err(Failure::Retry(
            "waiting for the attach parent to record it".into(),
        )),
    }
}

/// client 에서 받은 원격 결과를 그 턴의 route 토큰이 가리키는 수신처로 넘길 `codex.deliver` 요청. 수신처가 없거나
/// 본문이 틀리면 거절 사유다.
pub(super) fn local_request(
    payload: &Value,
    surface: u32,
    receiver_id: Option<String>,
) -> Result<Value, String> {
    let mut request = payload
        .get(PAYLOAD_KEY)
        .filter(|c| c.is_object())
        .cloned()
        .ok_or("payload without a completion")?;
    let key = super::key_of(&request)
        .ok_or("completion without completion_key")?
        .to_string();
    let receiver_id = receiver_id
        .filter(|r| !r.is_empty())
        .ok_or_else(|| format!("no Codex return route for the result of s{surface}"))?;
    let map = request
        .as_object_mut()
        .ok_or("completion is not an object")?;
    map.remove("attach");
    map.remove(remote_attach::ROUTE_FIELD);
    map.insert("receiver_id".into(), Value::from(receiver_id));
    map.insert(
        "completion_key".into(),
        Value::from(remote_attach::local_completion_key(&key)),
    );
    map.insert("child_surface".into(), Value::from(surface));
    Ok(request)
}

/// 원격 자식에게 보낸 지시 하나의 route 토큰. 서버가 그 지시로 시작한 턴의 결과에 이 토큰을 실어 돌려준다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AttachRoute {
    pub(super) token: String,
    pub(super) receiver_id: String,
    pub(super) issued_at_ms: u64,
}

/// route 토큰 기록을 두는 기간. 서버는 결과를 확인받을 때까지 다시 보내며 host 통지 보관은 24시간이다.
const ATTACH_ROUTE_KEPT: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// route 토큰 기록 상한. 넘으면 오래된 것부터 지운다.
const ATTACH_ROUTES_MAX: usize = 4096;

impl Returns {
    /// client: 서버에서 온 원격 자식 결과를 그 mirror 자식 경로의 수신처로 기록한다(`claude.remote_receive`).
    /// 같은 결과의 재전송은 같은 키라 Codex 원장이 중복으로 거른다. 기록하지 못하면 오류로 답해 host 가
    /// 나중에 다시 보내게 한다. 경로는 mirror 가 사라져도 남아 있으므로 늦게 온 결과도 기록한다.
    pub(crate) fn remote_receive<H: HostCall>(
        &self,
        host: &H,
        params: &Value,
    ) -> Result<Value, IpcMethodError> {
        use tasty_plugin_agent_common::remote_attach::{Received, received};
        let Some((surface, payload)) = received(params) else {
            return Ok(Received::Rejected("missing surface_id or payload".into()).json());
        };
        // 수신처는 지금의 경로가 아니라 그 턴을 지시한 spawn·tell 때 발급한 토큰으로 찾는다. 그 사이 경로가
        // 풀리거나 다른 부모로 묶여도 그 턴의 결과는 지시한 부모에게 간다.
        let token = payload
            .get(PAYLOAD_KEY)
            .and_then(|c| c.get(remote_attach::ROUTE_FIELD))
            .and_then(Value::as_str)
            .filter(|t| remote_attach::is_route_token(t));
        let receiver = match token {
            Some(token) if self.is_available() => self
                .update(|book| {
                    let r = book
                        .attach_routes
                        .iter()
                        .find(|r| r.token == token)
                        .map(|r| r.receiver_id.clone());
                    (r, false)
                })
                .map_err(IpcMethodError::new)?,
            _ => None,
        };
        let request = match local_request(payload, surface, receiver) {
            Ok(r) => r,
            Err(reason) => return Ok(Received::Rejected(reason).json()),
        };
        let response = host
            .call_within("codex.deliver", request, DELIVER_CALL_TIMEOUT)
            .map_err(IpcMethodError::from)?;
        let status = if response.get("duplicate").and_then(Value::as_bool) == Some(true) {
            Received::Duplicate
        } else {
            Received::Recorded
        };
        Ok(status.json())
    }

    /// client: 원격 자식에게 보내는 spawn·tell 하나의 route 토큰을 발급한다. 토큰→수신처 기록은 경로를 풀거나 다시
    /// 묶어도 남고 [`ATTACH_ROUTE_KEPT`] 뒤에 지운다. 서버에는 토큰만 보낸다.
    pub(crate) fn issue_attach_route(&self, receiver_id: &str) -> Result<String, String> {
        if !self.is_available() {
            return Err("no plugin data directory".into());
        }
        let token = remote_attach::new_route_token();
        let now = now_ms();
        self.update(|book| {
            book.attach_routes
                .retain(|r| now.saturating_sub(r.issued_at_ms) <= ms(ATTACH_ROUTE_KEPT));
            book.attach_routes.push(AttachRoute {
                token: token.clone(),
                receiver_id: receiver_id.to_string(),
                issued_at_ms: now,
            });
            if book.attach_routes.len() > ATTACH_ROUTES_MAX {
                let extra = book.attach_routes.len() - ATTACH_ROUTES_MAX;
                book.attach_routes.drain(..extra);
            }
            ((), true)
        })?;
        Ok(token)
    }

    /// client: 원격 자식이 닫혔다. 서버에 남은 결과가 늦게 올 수 있으므로 경로를 바로 지우지 않고 끝난 경로로
    /// 두어 [`ENDED_ROUTE_GRACE`] 뒤 지운다.
    pub(crate) fn end_remote_child(&self, child_surface: u32) {
        if !self.is_available() {
            return;
        }
        let now = now_ms();
        let result = self.update(|book| {
            let mut changed = false;
            for r in book
                .routes
                .iter_mut()
                .filter(|r| r.child_surface == child_surface && r.ended_at_ms.is_none())
            {
                r.ended_at_ms = Some(now);
                changed = true;
            }
            ((), changed)
        });
        if let Err(e) = result {
            tracing::warn!(
                "claude codex return: could not end the remote child s{child_surface}: {e}"
            );
        }
    }
}
