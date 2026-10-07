# ADR-0068: v2 task 그래프는 전체 검증 뒤 그래프 레코드 하나로 활성화한다

- **Status**: Accepted — 3단계를 목록 한 번으로 평가하게 바꾼 뒤 재측정해 상한을 1000 으로 올렸다(2026-10-07, 재검토 조건 3)
- **Date**: 2026-10-06
- **Tags**: agents, tasks, types, dag, storage
- **Group**: agents

## Context

v2 task 의 입력 binding 은 다른 task 의 출력을 타입으로 받는다. 받는 쪽과 내는 쪽이 서로를 참조하므로, task 를 하나씩 만들면 일부만 만들어진 그래프의 task 가 러너에서 먼저 실행될 수 있다. 중간에 생성이 실패하면 실행된 앞부분은 되돌릴 수 없다. 메모리 저장소는 여러 키를 한 번에 쓰는 트랜잭션을 제공하지 않고, 러너는 별도 스레드에서 저장소 목록을 읽어 Ready task 를 실행한다.

## Decision

v2 task 는 그래프 단위로 제출한다(`agent.task_graph_submit`, 검증만 하는 `agent.task_graph_validate`). 순서는 세 단계다.

1. 그래프 전체를 검증한다. task 수 상한, id, 참조, 타입, binding, 매핑, 조합 규칙, 순환을 확인하고 실패하면 아무것도 쓰지 않는다. 오류에는 제출 정의 안의 JSON Pointer 를 싣는다.
2. task 를 모두 Waiting 으로 쓴다. 쓰는 도중 실패하면 쓴 task 를 지운다.
3. 그래프 레코드(`tasty.agent.task_graph.<그래프 id>`, `tasty.task_graph/v1`) 하나를 쓰고 readiness 를 평가한다.

`graph_id` 를 가진 task 는 그 그래프 레코드가 없으면 readiness 평가에서 항상 대기한다. 그래서 2단계 중에 러너 tick 이 돌아도 실행되지 않고, 활성화는 레코드 한 키의 쓰기로 정해진다. 그래프의 task 가 모두 지워지면 레코드도 지운다. 운영 규칙은 [작업 러너 §그래프 제출](../dev-guide/agent-runner.md#그래프-제출).

## Consequences

- 검증 실패는 아무것도 쓰지 않는다. 2단계(task 기록)와 그래프 레코드 쓰기의 저장 실패는 쓴 task 를 지워 남는 task 가 없다. 러너 tick 이 끼어도 활성화 전 task 는 실행되지 않으므로, 활성화의 정확성은 잠금에 기대지 않는다.
- 3단계(readiness 반영)는 레코드를 쓴 뒤라 롤백하지 않는다. 이 단계의 저장 실패는 그래프가 이미 활성일 수 있다는 표지와 함께 오류로 돌려준다. 이미 Ready 가 된 task 는 러너가 실행하고, 같은 id 로 다시 제출하면 거절된다.
- 3단계 도중 호스트가 죽어도 같다. 반영하지 못한 의존 없는 task 는 Waiting 으로 남는다. 결정 당시에는 재시작이 readiness 를 다시 평가하지 않아 수동 복구가 필요했다. 2026-10-07 부터 부팅·러너 시작이 Waiting task 를 다시 평가해 남은 task 를 활성화한다([ADR-0069](0069-typed-task-completion-is-one-write-per-attempt.md) 과 같은 변경). 다시 제출하는 절차는 [작업 러너 §한계](../dev-guide/agent-runner.md#한계).
- 2단계에서 롤백까지 실패하면 활성화되지 않은 task 가 남는다. 실행되지 않으며 삭제·purge 로 지운다.
- readiness 평가는 task 마다 그래프 레코드 존재를 확인해야 한다(`TaskStore::readiness_graph`).
- 제출 전체가 memory 잠금 안에서 앱의 IPC 처리 경로(main 스레드의 IPC 큐) 위에서 돈다. 그동안 러너 tick 과 같은 경로의 다른 IPC 요청 전체가 기다린다. 그래서 그래프의 task 수에 상한을 둔다. 상한은 같은 머신의 제출 시간과 제출 중 다른 IPC 의 지연을 재서 정했고, 상한 크기의 제출 동안 다른 IPC 가 최악 약 1.4s 멈출 수 있다는 지연을 받아들인다. 처음 구현은 3단계가 task 마다 저장소를 다시 읽어 상한을 200 으로 두었고, 목록을 한 번 읽게 바꾼 뒤 재측정해 1000 으로 올렸다. 측정값은 [작업 러너 §그래프 제출](../dev-guide/agent-runner.md#그래프-제출)에 있다.

### 영속이 아닌 저장소에서의 활성화

memory 파일을 열지 못하면 Tasty 는 멈추지 않고 임시 메모리로 계속 동작한다(대체 모드). 쓰기 응답은 `durable: false` 를 싣지만 쓰기 자체는 받아들인다([ADR-0010](0010-storage-failure-reporting.md)). v2 그래프는 재시작 뒤에도 이어서 실행하는 것을 전제로 만드는데, 대체 모드에서 그대로 받으면 호출자는 복구되는 그래프를 보냈다고 믿지만 재시작하면 그래프·결과·회차가 모두 사라진다. 응답의 `durable: false` 는 이미 실행을 시작한 뒤에야 보인다.

그래서 그래프 정의에 `durability`(`required` 기본, `best_effort`)를 둔다.

- 대체 모드에서 `required` 그래프는 제출과 검증 모두 거절하고 아무것도 저장하지 않는다. 오류 데이터에 대체 모드의 원인을 싣는다.
- `best_effort` 그래프는 대체 모드에서도 실행하며 재시작 복구를 약속하지 않는다.
- 그래프 레코드와 제출·검증 응답에 `durability` 를 남긴다.
- 판정은 `TaskService::task_graph_submit` 이 한다. 서비스는 조립할 때 대체 모드의 원인을 받으므로 IPC 처리기를 거치지 않는 Rust 호출자도 같은 판정을 받는다.
- v1 task 와 단건 생성은 바꾸지 않는다. 기존 동작(받고 `durable: false` 로 알림)을 유지한다.

그래프를 보내는 쪽이 재시작 복구 여부를 실행 전에 안다. 정상 모드에서는 동작이 같다. 대체 모드를 모르는 도구는 거절을 받고 원인을 오류 데이터에서 읽는다. task 조회(`task_get`·`task_list`)는 그래프의 `durability` 를 싣지 않는다.

## Alternatives Considered

- **task 를 하나씩 만들고 앞 task 를 예약 상태로 두기**: v1 의 `reserved_for_fallback` 과 같은 방식이다. 예약을 푸는 쓰기가 task 마다 필요해 활성화가 원자적이지 않다.
- **러너를 멈추거나 저장소 잠금을 잡고 쓰기**: 러너가 workspace 별 스레드라 모든 경로가 같은 잠금을 따라야 하고, 잠금을 쥔 채 실패하면 러너가 멈춘다.
- **그래프 전체를 레코드 하나로 저장하기**: 기존 task 단위 조회·삭제·재시도·DAG 화면이 모두 task 키를 읽으므로 그래프 안의 task 를 따로 다루는 경로가 생긴다.
- **`durability` 기본값을 `best_effort` 로 둔다.** 대체 모드를 모르는 호출자가 계속 복구되지 않는 그래프를 만든다. 문제를 조용히 넘기는 쪽이라 택하지 않았다.
- **대체 모드에서 모든 그래프를 거절한다.** 임시 실행으로 충분한 작업(테스트·일회성 확인)까지 막는다.
- **v1 단건 생성도 같은 규칙으로 막는다.** 기존 호출자의 동작을 바꾸므로 제외했다. v1 은 재시작 복구를 계약으로 내세우지 않는다.

## Reconsideration Triggers

- 메모리 저장소가 여러 키 쓰기를 한 트랜잭션으로 제공하면 2·3단계를 하나로 합친다(`tasty_memory` 의 쓰기 API).
- 재시작 시 readiness 를 다시 평가하는 경로가 생기면 Consequences 의 남는 Waiting 을 지운다(`TaskStore` 의 복구 경로).
- readiness 그래프를 task 마다 다시 만들지 않도록 바꾸면 제출 시간을 다시 재고 `MAX_GRAPH_TASKS` 를 조정한다(`TaskStore::settle_waiting`).
- 대체 모드가 실행 중에 바뀌게 되면(재연결로 파일 저장소 복귀 등) 조립 때 받은 원인 대신 저장소 상태를 매번 읽어야 한다.
- 대체 모드가 사라지거나(초기화 실패 시 시작을 막는 정책) 다른 저장소가 생기면 `durability` 구분을 다시 본다.
- 사용자가 `best_effort` 를 기본처럼 붙여 쓰는 사례가 많으면 기본값과 오류 안내를 다시 정한다(실행 결과로 확인).

## References

- [ADR-0067](0067-typed-task-contracts-live-in-a-separate-record-namespace.md) — v2 task 의 저장 namespace 와 envelope.
- [ADR-0042](0042-agent-coordination-and-task-views.md) — 작업 조율과 DAG 화면.
- [ADR-0010](0010-storage-failure-reporting.md) — 대체 모드의 쓰기 응답.
- `crates/tasty-agent/src/task/store/graph_submit.rs` — 제출 단계, `GraphDurability`, 그래프 레코드.
- `crates/tasty-task-runtime/src/task.rs` — `TaskService::task_graph_submit` 의 대체 모드 판정. `src/adapters/ipc/handler/agent.rs` — `StoreNotDurable` 의 IPC 응답.
