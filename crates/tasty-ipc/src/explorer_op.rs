//! 원격 explorer 파일 작업의 제어 메시지. attach holder 가 서버 파일을 바꾸게 요청하고,
//! 서버는 자기 작업 엔진으로 실행하면서 진행·충돌 질문·결과를 같은 연결로 돌려준다.
//!
//! 작업 내용(`op`)과 진행·결과의 모양은 작업 엔진을 가진 호스트가 정하므로 여기서는 JSON 값으로 둔다.
//! 허브는 event 태그로 분류만 한다. 옛 서버는 이 메시지를 모르고 버리므로 client 는
//! [`CAPABILITY`] 를 attach descriptor 에서 확인한 연결에만 보낸다.

use serde::{Deserialize, Serialize};

/// 서버가 원격 explorer 파일 작업을 받는다는 capability 이름. attach descriptor 의
/// [`DESCRIPTOR_CAPABILITIES`](crate::stream::DESCRIPTOR_CAPABILITIES) 와 system.info 에 싣는다.
pub const CAPABILITY: &str = "ipc.stream.explorer-write";
/// [`CAPABILITY`] 의 버전.
pub const VERSION: u32 = 1;

/// 서버 → client 진행 통지의 event 이름. `op_id`·`progress`·`ask`(없으면 null)를 싣는다.
pub const PROGRESS_EVENT: &str = "explorer_op_progress";
/// 서버 → client 결과의 event 이름. `op_id` 와 `report`, 또는 `error`·`reason` 을 싣는다.
pub const RESULT_EVENT: &str = "explorer_op_result";

/// client → 서버 요청.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ExplorerOpMsg {
    /// 작업 하나를 시작한다. `op_id` 는 연결 안에서 client 가 고른다.
    ExplorerOpRequest { op_id: u64, op: serde_json::Value },
    /// 기다리는 충돌 질문에 답한다. 질문이 없으면 서버가 버린다.
    ExplorerOpAnswer {
        op_id: u64,
        choice: ConflictChoice,
        apply_all: bool,
    },
    /// 남은 항목을 멈춘다. 끝난 작업이면 서버가 버린다.
    ExplorerOpCancel { op_id: u64 },
}

/// 이름 충돌의 답.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictChoice {
    KeepBoth,
    Skip,
    Replace,
    CancelRest,
}

/// 작업을 시작하지 못한 이유. `explorer_op_result` 의 `error` 값이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusedOp {
    /// 요청한 연결이 이 서버의 workspace 를 점유하고 있지 않다.
    NotHolder,
    /// 이 연결에서 이미 작업이 실행 중이다. client 는 한 번에 하나만 보낸다.
    Busy,
    /// 작업 내용을 읽지 못했다.
    Invalid,
    /// 작업 스레드를 만들지 못했다.
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_with_their_event_tags() {
        let answer = ExplorerOpMsg::ExplorerOpAnswer {
            op_id: 3,
            choice: ConflictChoice::CancelRest,
            apply_all: true,
        };
        let json = serde_json::to_value(&answer).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "event": "explorer_op_answer", "op_id": 3,
                "choice": "cancel_rest", "apply_all": true,
            })
        );
        assert_eq!(
            serde_json::from_value::<ExplorerOpMsg>(json).unwrap(),
            answer
        );
        let request: ExplorerOpMsg = serde_json::from_value(serde_json::json!({
            "event": "explorer_op_request", "op_id": 1, "op": {"kind": "trash"},
        }))
        .unwrap();
        assert!(matches!(
            request,
            ExplorerOpMsg::ExplorerOpRequest { op_id: 1, .. }
        ));
    }

    /// 다른 Control 메시지와 event 이름이 겹치지 않아야 허브가 잘못 분류하지 않는다.
    #[test]
    fn other_control_messages_are_not_explorer_ops() {
        let list_dir = serde_json::json!({"event": "list_dir_request", "request_id": 1, "dir": ""});
        assert!(serde_json::from_value::<ExplorerOpMsg>(list_dir).is_err());
    }
}
