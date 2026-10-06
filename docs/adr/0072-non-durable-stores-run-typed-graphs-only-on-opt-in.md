# ADR-0072: 영속이 아닌 저장소에서는 명시 허용한 v2 그래프만 활성화한다

- **Status**: Accepted
- **Date**: 2026-10-07
- **Tags**: agents, tasks, dag, storage, durability
- **Group**: agents

## Context

memory 파일을 열지 못하면 Tasty 는 멈추지 않고 임시 메모리로 계속 동작한다(대체 모드). 이때 쓰기 응답은 `durable: false` 를 싣지만 쓰기 자체는 받아들인다([ADR-0010](0010-storage-failure-reporting.md)). v2 그래프는 여러 task 와 그 입력·결과를 재시작 뒤에도 이어서 실행하는 것을 전제로 만든다. 대체 모드에서 그대로 받으면 호출자는 재시작 복구가 되는 그래프를 보냈다고 믿지만, 재시작하면 그래프·결과·회차가 모두 사라진다. 응답의 `durable: false` 는 이미 실행을 시작한 뒤에야 보인다.

## Decision

v2 그래프 정의에 `durability` 를 둔다. 값은 `required`(기본)와 `best_effort` 다.

- 대체 모드에서 `required` 그래프는 `agent.task_graph_submit`·`agent.task_graph_validate` 모두 `-32602` 로 거절한다. `error.data` 는 `location: /durability`, `store_durable: false`, 대체 모드의 `cause` 를 싣는다. 아무것도 저장하지 않는다.
- `best_effort` 그래프는 대체 모드에서도 실행한다. 재시작 복구는 약속하지 않는다.
- 그래프 레코드와 제출·검증 응답에 `durability` 를 남긴다. 대체 모드의 제출 응답은 다른 쓰기처럼 `durable: false` 도 싣는다.
- v1 task 와 단건 생성은 바꾸지 않는다. 기존 동작(받고 `durable: false` 로 알림)을 유지한다.

현재 동작은 [agent runner 가이드](../dev-guide/agent-runner.md)의 "그래프 제출" 절에 있다.

## Consequences

- 그래프를 보내는 쪽이 재시작 복구 여부를 실행 전에 안다. 대체 모드를 모르는 도구는 거절을 받고 원인을 `error.data` 에서 읽는다.
- 정상 모드에서는 동작이 같다. `durability` 를 적지 않은 그래프는 그대로 실행된다.
- 판정은 `TaskService::task_graph_submit` 이 한다(`AgentError::StoreNotDurable`). 서비스는 조립할 때 대체 모드의 원인을 받으므로(`with_store_fallback`), IPC 처리기를 거치지 않는 Rust 호출자도 같은 판정을 받는다. IPC 처리기는 그 오류를 `-32602` 로 전달한다.
- task 조회(`task_get`·`task_list`)는 그래프의 `durability` 를 따로 싣지 않는다. 그래프 레코드에만 있다.

## Alternatives Considered

- **기본값을 `best_effort` 로 둔다.** 대체 모드를 모르는 호출자가 계속 복구되지 않는 그래프를 만든다. 문제를 조용히 넘기는 쪽이라 택하지 않았다.
- **대체 모드에서 모든 그래프를 거절한다.** 임시 실행으로 충분한 작업(테스트·일회성 확인)까지 막는다.
- **v1 단건 생성도 같은 규칙으로 막는다.** 기존 호출자의 동작을 바꾸므로 이번 범위에서 제외했다. v1 은 재시작 복구를 계약으로 내세우지 않는다.

## Reconsideration Triggers

코드와 설정에서 확인:

- 대체 모드가 실행 중에 바뀌게 되면(재연결로 파일 저장소 복귀 등) 조립 때 받은 원인 대신 저장소 상태를 매번 읽어야 한다.
- 대체 모드가 사라지거나(초기화 실패 시 시작을 막는 정책) 다른 저장소가 생기면 이 구분을 다시 본다.

실행 결과로 확인:

- 사용자가 `best_effort` 를 기본처럼 붙여 쓰는 사례가 많으면 기본값과 오류 안내를 다시 정한다.

## References

- `crates/tasty-task-runtime/src/task.rs` — `TaskService::task_graph_submit` 의 판정.
- `src/adapters/ipc/handler/agent.rs` — `StoreNotDurable` 의 IPC 응답.
- `crates/tasty-agent/src/task/store/graph_submit.rs` — `GraphDurability`, 그래프 레코드.
- [ADR-0010](0010-storage-failure-reporting.md) — 대체 모드의 쓰기 응답.
- [ADR-0069](0069-typed-task-graphs-activate-through-a-graph-record.md) — 그래프 활성화.
