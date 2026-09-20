//! 서버가 **무엇을 협상할 수 있는지** 이름과 판으로 선언한다.
//!
//! 이 목록이 답하는 물음은 하나다 — *구 client 가 이 응답을 이해하나.* 지금까지 그것을
//! 물어볼 자리가 RPC 쪽에 없었다. 봉투 타입([`crate::protocol`])에는 판 필드가 없고,
//! `system.info` 가 내던 버전은 패키지 버전 하나라 "이 서버가 무엇을 할 줄 아는가" 를
//! 말하지 않았다(패키지 버전은 기능 목록이 아니다 — 같은 버전의 두 빌드가 feature 조합에
//! 따라 다른 것을 할 수 있다).
//!
//! stream 쪽에는 이미 답이 있다. [`crate::stream::STREAM_PROTO`] 를 서버가 handshake 에서
//! 동등 비교하고, 거절 ack 에 자기 판을 실어 보낸다. 이 모듈은 **그 모양을 RPC 쪽으로
//! 옮긴 것이 아니라**, RPC 쪽에서 같은 물음에 답할 자리를 만든 것이다 — 새 handshake 를
//! 만들지 않고 이미 있는 `system.info` 응답에 키를 하나 더한다. 구 client 는 모르는 키를
//! 무시하므로 부수효과가 없다.
//!
//! **없는 기능을 적지 않는다.** 목록의 각 항목은 이 트리에 근거가 있는 것뿐이고, 판은
//! 가능하면 그 근거에서 **유도한다**(리터럴로 다시 적으면 근거와 갈린다).

/// 서버가 선언하는 협상 가능한 기능 하나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    /// 점으로 구분한 안정 이름. 한 번 발행하면 뜻이 안 바뀐다 — 뜻이 바뀌면 판을 올린다.
    pub name: &'static str,
    /// 그 기능의 판. 올라가는 방향으로만 움직인다.
    pub version: u32,
}

/// 이 빌드가 선언하는 기능 목록.
///
/// 항목을 더하는 것은 **추가**라 구 client 에 영향이 없다. 반대로 **빼거나 뜻을 바꾸는
/// 것은 표면 축소**이고, 그때 움직이는 것은 이 배열이 아니라 그 항목의 `version` 이다.
pub const CAPABILITIES: &[Capability] = &[
    // 이 선언 자체. client 가 이 이름을 보면 "이 서버에는 물어볼 수 있다" 를 안다 —
    // 키가 아예 없는 구 서버와 구별되는 유일한 표지다.
    Capability {
        name: "ipc.capabilities",
        version: 1,
    },
    // 표가 메서드마다 "두 번 전달되면 무엇이 남는가" 를 답한다.
    // 근거: `crate::method_meta::MethodEffect`.
    Capability {
        name: "ipc.method-effect",
        version: 1,
    },
    // 표가 메서드마다 "0.7.0 동결 표면에 있었나" 를 답한다.
    // 근거: `crate::method_meta::MethodSince`.
    Capability {
        name: "ipc.method-since",
        version: 1,
    },
    // 봉투가 응답 대기 상한을 실을 수 있다. 근거: `crate::protocol::JsonRpcRequest` 의
    // `response_timeout_ms` 와 그것을 읽는 서버의 대기 자리. 이 이름이 없는 서버는 그
    // 필드를 조용히 버리므로(봉투에 `deny_unknown_fields` 가 없다) client 는 **보내기
    // 전에** 이 이름으로 물어야 상한이 실제로 걸리는지 안다.
    Capability {
        name: "ipc.response-timeout",
        version: 1,
    },
    // 봉투가 멱등 키를 실을 수 있다 — 이 서버가 그 필드를 **읽는다**는 선언이다.
    // 근거: `crate::protocol::JsonRpcRequest::idempotency_key` 와 그것을 읽는 호스트의
    // 보존소. `ipc.response-timeout` 과 같은 이유로 이름이 필요하다 — 이 이름이 없는
    // 서버는 키를 조용히 버리므로, 보내는 것만으로는 계약이 걸렸는지 알 수 없다.
    // 그리고 이 계약은 **부수효과가 남는 메서드**에 쓰이므로 확인이 늦으면 늦은 만큼
    // 두 번째 효과가 이미 남는다.
    //
    // ★ 이 이름은 **메서드 단위 보장을 뜻하지 않는다.** 호스트의 보존소는 engine
    // 라우터 한 자리에 있고, 그 앞에서 끝나는 메서드(App 층 · plugin namespace
    // forward)는 키를 실어도 그냥 실행된다. 그 차이를 말하는 이름은 아직 없다 —
    // 만들려면 그 자리들이 먼저 배선돼야 하고, 배선 전에 선언하면 "착지한 것만
    // 적는다" 를 어긴다.
    Capability {
        name: "ipc.idempotency-key",
        version: 1,
    },
    // 스트리밍 채널의 프레임 프로토콜. 판을 **리터럴로 안 적는다** — 서버가 handshake 에서
    // 동등 비교하는 그 상수를 그대로 싣는다. 둘로 적으면 갈린다.
    Capability {
        name: "ipc.stream",
        version: crate::stream::STREAM_PROTO,
    },
    // 스트림이 프레임을 버렸을 때 그 사실을 client 에 알릴 수 있다
    // (`crate::stream::StreamControl::Loss`). **`ipc.stream` 의 판으로는 이것을 못
    // 말한다** — 그 판은 서버가 handshake 에서 **동등 비교**하는 수라, 올리면 기능이
    // 좁아지는 것이 아니라 구 peer 의 연결이 거절된다. 그래서 더해지는 스트림 기능은
    // 판이 아니라 **이름**으로 선언한다. 항목 추가는 구 client 에 영향이 없다(위 문단).
    Capability {
        name: "ipc.stream.loss-notify",
        version: 1,
    },
];

/// `system.info` 가 싣는 모양. `[{ "name": …, "version": … }, …]`.
pub fn capabilities_json() -> serde_json::Value {
    serde_json::Value::Array(
        CAPABILITIES
            .iter()
            .map(|c| serde_json::json!({ "name": c.name, "version": c.version }))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이름이 겹치면 client 가 둘 중 어느 판을 믿을지 알 수 없다.
    #[test]
    fn no_capability_name_is_declared_twice() {
        let mut names: Vec<&str> = CAPABILITIES.iter().map(|c| c.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "중복된 capability 이름: {names:?}");
    }

    /// 판 0 은 "없다" 와 구별되지 않는다 — JSON 에서 빠진 키와 같게 읽힌다.
    #[test]
    fn every_declared_version_starts_at_one() {
        for c in CAPABILITIES {
            assert!(c.version >= 1, "{} 의 판이 0 이다", c.name);
        }
    }

    /// stream 판은 **서버가 비교하는 그 상수**여야 한다. 리터럴로 다시 적으면 여기서
    /// 갈린다 — 이 시험이 그 갈림을 잡는 유일한 자리다.
    #[test]
    fn the_stream_capability_carries_the_constant_the_server_compares() {
        let declared = CAPABILITIES
            .iter()
            .find(|c| c.name == "ipc.stream")
            .expect("stream capability 가 사라졌다");
        assert_eq!(declared.version, crate::stream::STREAM_PROTO);
    }

    /// 실릴 모양이 배열이고, 각 항목이 두 키를 든다.
    #[test]
    fn the_json_shape_is_an_array_of_name_and_version() {
        let v = capabilities_json();
        let arr = v.as_array().expect("배열이어야 한다");
        assert_eq!(arr.len(), CAPABILITIES.len());
        for item in arr {
            assert!(item["name"].is_string(), "{item}");
            assert!(item["version"].is_u64(), "{item}");
        }
    }
}
