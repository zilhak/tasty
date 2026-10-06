# ADR-0069: v2 task 그래프는 전체 검증 뒤 그래프 레코드 하나로 활성화한다

- **Status**: Accepted
- **Date**: 2026-10-06
- **Tags**: agents, tasks, types, dag, storage
- **Group**: agents

## Context

v2 task 의 입력 binding 은 다른 task 의 출력을 타입으로 받는다. 받는 쪽과 내는 쪽이 서로를 참조하므로, task 를 하나씩 만들면 일부만 만들어진 그래프의 task 가 러너에서 먼저 실행될 수 있다. 중간에 생성이 실패하면 실행된 앞부분은 되돌릴 수 없다. 메모리 저장소는 여러 키를 한 번에 쓰는 트랜잭션을 제공하지 않고, 러너는 별도 스레드에서 저장소 목록을 읽어 Ready task 를 실행한다.

## Decision

v2 task 는 그래프 단위로 제출한다(`agent.task_graph_submit`, 검증만 하는 `agent.task_graph_validate`). 순서는 세 단계다.

1. 그래프 전체를 검증한다. task 수(상한 `MAX_GRAPH_TASKS` = 200), id, 참조, 타입, binding, 매핑, 조합 규칙, 순환을 확인하고 실패하면 아무것도 쓰지 않는다. 오류에는 제출 정의 안의 JSON Pointer 를 싣는다.
2. task 를 모두 Waiting 으로 쓴다. 쓰는 도중 실패하면 쓴 task 를 지운다.
3. 그래프 레코드(`tasty.agent.task_graph.<그래프 id>`, `tasty.task_graph/v1`) 하나를 쓰고 readiness 를 평가한다.

`graph_id` 를 가진 task 는 그 그래프 레코드가 없으면 readiness 평가에서 항상 대기한다. 그래서 2단계 중에 러너 tick 이 돌아도 실행되지 않고, 활성화는 레코드 한 키의 쓰기로 정해진다. 그래프의 task 가 모두 지워지면 레코드도 지운다. 운영 규칙은 [작업 러너 §그래프 제출](../dev-guide/agent-runner.md#그래프-제출).

## Consequences

- 검증 실패는 아무것도 쓰지 않는다. 2단계(task 기록)와 그래프 레코드 쓰기의 저장 실패는 쓴 task 를 지워 남는 task 가 없다. 러너 tick 이 끼어도 활성화 전 task 는 실행되지 않으므로, 활성화의 정확성은 잠금에 기대지 않는다.
- 3단계(readiness 반영)는 레코드를 쓴 뒤라 롤백하지 않는다. 이 단계의 저장 실패는 오류(`AgentError::GraphPartiallyActivated`, IPC `-32603`)로 돌려주고 `error.data` 에 `graph_id` 와 `possibly_active: true` 를 싣는다. 그래프는 활성이며 이미 Ready 가 된 task 는 러너가 실행한다. 같은 id 로 다시 제출하면 거절된다.
- 3단계 도중 호스트가 죽어도 같다. 반영하지 못한 의존 없는 task 는 Waiting 으로 남고 재시작은 readiness 를 다시 평가하지 않는다. 복구 절차는 [작업 러너 §한계](../dev-guide/agent-runner.md#한계).
- 2단계에서 롤백까지 실패하면 활성화되지 않은 task 가 남는다. 실행되지 않으며 삭제·purge 로 지운다.
- readiness 평가는 task 마다 그래프 레코드 존재를 확인해야 한다(`TaskStore::readiness_graph`).
- 제출 전체가 memory 잠금 안에서 돈다. 3단계가 task 마다 workspace 목록과 readiness 그래프를 다시 만들어 비용이 task 수의 제곱으로 늘어난다. 그동안 memory 를 쓰는 다른 IPC 와 러너 tick 이 기다린다. 실측(빈 workspace, 러너 정지, 부하 있는 머신)은 50 개 39ms, 200 개 430ms, 1000 개 5.7s 였고, 1000 개 제출 중 다른 연결의 `memory.get` 은 5369ms 를 기다렸다. 그래서 그래프 하나를 200 개로 제한하고 초과는 `-32602`(`location: /tasks`)로 거절한다.

## Alternatives Considered

- **task 를 하나씩 만들고 앞 task 를 예약 상태로 두기**: v1 의 `reserved_for_fallback` 과 같은 방식이다. 예약을 푸는 쓰기가 task 마다 필요해 활성화가 원자적이지 않다.
- **러너를 멈추거나 저장소 잠금을 잡고 쓰기**: 러너가 workspace 별 스레드라 모든 경로가 같은 잠금을 따라야 하고, 잠금을 쥔 채 실패하면 러너가 멈춘다.
- **그래프 전체를 레코드 하나로 저장하기**: 기존 task 단위 조회·삭제·재시도·DAG 화면이 모두 task 키를 읽으므로 그래프 안의 task 를 따로 다루는 경로가 생긴다.

## Reconsideration Triggers

- 메모리 저장소가 여러 키 쓰기를 한 트랜잭션으로 제공하면 2·3단계를 하나로 합친다(`tasty_memory` 의 쓰기 API).
- 재시작 시 readiness 를 다시 평가하는 경로가 생기면 Consequences 의 남는 Waiting 을 지운다(`TaskStore` 의 복구 경로).
- 3단계를 workspace 목록 한 번으로 평가하도록 바꾸면 제출 시간을 다시 재고 `MAX_GRAPH_TASKS` 를 올린다(`graph_submit.rs`).

## References

- [ADR-0068](0068-typed-task-contracts-live-in-a-separate-record-namespace.md) — v2 task 의 저장 namespace 와 envelope.
- [ADR-0042](0042-agent-coordination-and-task-views.md) — 작업 조율과 DAG 화면.
- `crates/tasty-agent/src/task/store/graph_submit.rs`
