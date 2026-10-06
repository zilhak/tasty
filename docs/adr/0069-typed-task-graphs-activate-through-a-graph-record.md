# ADR-0069: v2 task 그래프는 전체 검증 뒤 그래프 레코드 하나로 활성화한다

- **Status**: Accepted
- **Date**: 2026-10-06
- **Tags**: agents, tasks, types, dag, storage
- **Group**: agents

## Context

v2 task 의 입력 binding 은 다른 task 의 출력을 타입으로 받는다. 받는 쪽과 내는 쪽이 서로를 참조하므로, task 를 하나씩 만들면 일부만 만들어진 그래프의 task 가 러너에서 먼저 실행될 수 있다. 중간에 생성이 실패하면 실행된 앞부분은 되돌릴 수 없다. 메모리 저장소는 여러 키를 한 번에 쓰는 트랜잭션을 제공하지 않고, 러너는 별도 스레드에서 저장소 목록을 읽어 Ready task 를 실행한다.

## Decision

v2 task 는 그래프 단위로 제출한다(`agent.task_graph_submit`, 검증만 하는 `agent.task_graph_validate`). 순서는 세 단계다.

1. 그래프 전체를 검증한다. id, 참조, 타입, binding, 매핑, 조합 규칙, 순환을 확인하고 실패하면 아무것도 쓰지 않는다. 오류에는 제출 정의 안의 JSON Pointer 를 싣는다.
2. task 를 모두 Waiting 으로 쓴다. 쓰는 도중 실패하면 쓴 task 를 지운다.
3. 그래프 레코드(`tasty.agent.task_graph.<그래프 id>`, `tasty.task_graph/v1`) 하나를 쓰고 readiness 를 평가한다.

`graph_id` 를 가진 task 는 그 그래프 레코드가 없으면 readiness 평가에서 항상 대기한다. 그래서 2단계 중에 러너 tick 이 돌아도 실행되지 않고, 활성화는 레코드 한 키의 쓰기로 정해진다. 그래프의 task 가 모두 지워지면 레코드도 지운다. 운영 규칙은 [작업 러너 §그래프 제출](../dev-guide/agent-runner.md#그래프-제출).

## Consequences

- 검증 실패와 저장 실패 모두 실행된 task 를 남기지 않는다. 별도 잠금 없이 러너와 제출이 동시에 돌아도 된다.
- readiness 평가는 task 마다 그래프 레코드 존재를 확인해야 한다(`TaskStore::readiness_graph`).
- 3단계의 레코드 쓰기와 readiness 평가 사이에 호스트가 죽으면 의존이 없는 task 가 Waiting 으로 남는다. 재시작은 readiness 를 다시 평가하지 않는다.
- 2단계에서 롤백까지 실패하면 활성화되지 않은 task 가 남는다. 실행되지 않으며 삭제·purge 로 지운다.

## Alternatives Considered

- **task 를 하나씩 만들고 앞 task 를 예약 상태로 두기**: v1 의 `reserved_for_fallback` 과 같은 방식이다. 예약을 푸는 쓰기가 task 마다 필요해 활성화가 원자적이지 않다.
- **러너를 멈추거나 저장소 잠금을 잡고 쓰기**: 러너가 workspace 별 스레드라 모든 경로가 같은 잠금을 따라야 하고, 잠금을 쥔 채 실패하면 러너가 멈춘다.
- **그래프 전체를 레코드 하나로 저장하기**: 기존 task 단위 조회·삭제·재시도·DAG 화면이 모두 task 키를 읽으므로 그래프 안의 task 를 따로 다루는 경로가 생긴다.

## Reconsideration Triggers

- 메모리 저장소가 여러 키 쓰기를 한 트랜잭션으로 제공하면 2·3단계를 하나로 합친다(`tasty_memory` 의 쓰기 API).
- 재시작 시 readiness 를 다시 평가하는 경로가 생기면 Consequences 의 남는 Waiting 을 지운다(`TaskStore` 의 복구 경로).

## References

- [ADR-0068](0068-typed-task-contracts-live-in-a-separate-record-namespace.md) — v2 task 의 저장 namespace 와 envelope.
- [ADR-0042](0042-agent-coordination-and-task-views.md) — 작업 조율과 DAG 화면.
- `crates/tasty-agent/src/task/store/graph_submit.rs`
