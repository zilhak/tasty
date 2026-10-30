//! attach 원격 출처 부모의 턴 결과. 이 Tasty 는 부모의 수신처를 모르므로 결과를 서버 host 에 맡기고
//! (`attach.child_notice`), client 의 같은 plugin 이 그 부모의 수신처로 기록한다([`super::Returns::remote_receive`]).
//!
//! 영속 요청은 client 가 기록을 확인한(`delivered`) 뒤나 받을 수 없다고 한(`rejected`) 뒤에만 지운다.
//! `pending` 은 송신 실패와 같이 같은 키로 늘어나는 간격을 두고 다시 묻는다.

use serde_json::Value;
use tasty_plugin_agent_common::host_call::HostCall;
use tasty_plugin_agent_common::remote_attach::{self, NoticeStatus};

/// client 가 받을 결과 본문의 키. 나머지 형식은 이 plugin 이 정하며 host 는 해석하지 않는다.
pub(crate) const PAYLOAD_KEY: &str = "completion";

/// attach 원격 부모로 갈 요청인가.
pub(super) fn is_attach(request: &Value) -> bool {
    request.get("attach").and_then(Value::as_bool) == Some(true)
}

/// 요청을 서버 host 에 맡기거나 상태를 묻는다. 지워도 되면 `Ok`, 다시 물어야 하면 `Err` 다.
pub(super) fn hand_over<H: HostCall>(host: &H, mut request: Value) -> Result<Value, String> {
    let surface = request
        .get("child_surface")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("attach result without child_surface")?;
    let key = super::key_of(&request)
        .ok_or("attach result without completion_key")?
        .to_string();
    if let Some(map) = request.as_object_mut() {
        map.remove("attach");
    }
    let payload = serde_json::json!({ PAYLOAD_KEY: request });
    match remote_attach::child_notice(host, surface, &key, payload)? {
        NoticeStatus::Delivered => Ok(serde_json::json!({ "attach": "delivered" })),
        NoticeStatus::Rejected(reason) => {
            tracing::warn!(
                "claude codex return: {key} was refused by the attach parent and dropped: {reason}"
            );
            Ok(serde_json::json!({ "attach": "rejected", "reason": reason }))
        }
        NoticeStatus::Pending => Err("waiting for the attach parent to record it".into()),
    }
}

/// client 에서 받은 원격 결과를 이 mirror 자식 경로의 수신처로 넘길 `codex.deliver` 요청. 경로가 없거나
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
        .ok_or_else(|| format!("no Codex return route for s{surface}"))?;
    let map = request
        .as_object_mut()
        .ok_or("completion is not an object")?;
    map.remove("attach");
    map.insert("receiver_id".into(), Value::from(receiver_id));
    map.insert(
        "completion_key".into(),
        Value::from(remote_attach::local_completion_key(&key)),
    );
    map.insert("child_surface".into(), Value::from(surface));
    Ok(request)
}
