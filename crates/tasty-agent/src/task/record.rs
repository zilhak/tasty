//! [`Task`] 의 역직렬화 표현.
//!
//! v2 결과의 typed 값은 wire 형식만으로 int64 와 string 을 구별할 수 없어 스키마가 있어야
//! 읽을 수 있다. 같은 레코드의 계약에서 출력 스키마를 얻어 [`TypedResult::from_wire`] 로
//! 읽는다. 직렬화는 [`Task`] 의 필드 그대로다. 필드와 serde 속성은 [`Task`] 와 1:1 이며,
//! 변환이 모든 필드를 구조 분해와 리터럴로 나열하므로 한쪽에만 필드를 추가하면
//! 컴파일되지 않는다.

use serde::Deserialize;

use super::binding::{InputSnapshot, InputSnapshotWire};
use super::contract::TypedResultWire;
use super::{
    OnFailure, RouteDecision, SkipReason, Task, TaskAttempt, TaskCommand, TaskContract, TaskId,
    TaskResult, TaskState, TypedResult, WorkspaceId,
};

#[derive(Deserialize)]
pub(crate) struct TaskWire {
    id: TaskId,
    workspace_id: WorkspaceId,
    name: String,
    command: TaskCommand,
    #[serde(default)]
    depends_on: Vec<TaskId>,
    state: TaskState,
    created_at: u64,
    #[serde(default)]
    started_at: Option<u64>,
    #[serde(default)]
    finished_at: Option<u64>,
    #[serde(default)]
    result: Option<TaskResult>,
    #[serde(default)]
    on_failure: OnFailure,
    #[serde(default)]
    metadata: serde_json::Value,
    #[serde(default)]
    reserved_for_fallback: bool,
    #[serde(default)]
    contract: Option<TaskContract>,
    #[serde(default)]
    typed_result: Option<TypedResultWire>,
    #[serde(default)]
    graph_id: Option<String>,
    #[serde(default)]
    input_snapshot: Option<InputSnapshotWire>,
    #[serde(default)]
    accepted: Option<super::contract::AcceptedResponse>,
    #[serde(default)]
    attempt: Option<TaskAttempt>,
    #[serde(default)]
    route: Option<RouteDecision>,
    #[serde(default)]
    skip: Option<SkipReason>,
}

impl TryFrom<TaskWire> for Task {
    type Error = String;

    fn try_from(wire: TaskWire) -> Result<Self, String> {
        let TaskWire {
            id,
            workspace_id,
            name,
            command,
            depends_on,
            state,
            created_at,
            started_at,
            finished_at,
            result,
            on_failure,
            metadata,
            reserved_for_fallback,
            contract,
            typed_result,
            graph_id,
            input_snapshot,
            accepted,
            attempt,
            route,
            skip,
        } = wire;
        let input_snapshot = match (input_snapshot, &contract) {
            (None, _) => None,
            (Some(s), Some(c)) => Some(
                InputSnapshot::from_wire(s, &c.defs(), &c.input_schema())
                    .map_err(|e| format!("task {id}: stored input: {e}"))?,
            ),
            (Some(_), None) => {
                return Err(format!("task {id}: input_snapshot without a contract"));
            }
        };
        let typed_result = match (typed_result, &contract) {
            (None, _) => None,
            (Some(r), Some(c)) => Some(
                TypedResult::from_wire(r, &c.defs(), &c.output_schema(&command))
                    .map_err(|e| format!("task {id}: stored output: {e}"))?,
            ),
            (Some(_), None) => {
                return Err(format!("task {id}: typed_result without a contract"));
            }
        };
        Ok(Task {
            id,
            workspace_id,
            name,
            command,
            depends_on,
            state,
            created_at,
            started_at,
            finished_at,
            result,
            on_failure,
            metadata,
            reserved_for_fallback,
            contract,
            typed_result,
            graph_id,
            input_snapshot,
            accepted,
            attempt,
            route,
            skip,
        })
    }
}
