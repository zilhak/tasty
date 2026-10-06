# ADR-0069: v2 task 그래프는 전체 검증 뒤 그래프 레코드 하나로 활성화한다

- **Status**: Accepted — 3단계를 목록 한 번으로 평가하게 바꾼 뒤 재측정해 상한을 1000 으로 올렸다(2026-10-07, 재검토 조건 3)
- **Date**: 2026-10-06
- **Tags**: agents, tasks, types, dag, storage
- **Group**: agents

## Context

v2 task 의 입력 binding 은 다른 task 의 출력을 타입으로 받는다. 받는 쪽과 내는 쪽이 서로를 참조하므로, task 를 하나씩 만들면 일부만 만들어진 그래프의 task 가 러너에서 먼저 실행될 수 있다. 중간에 생성이 실패하면 실행된 앞부분은 되돌릴 수 없다. 메모리 저장소는 여러 키를 한 번에 쓰는 트랜잭션을 제공하지 않고, 러너는 별도 스레드에서 저장소 목록을 읽어 Ready task 를 실행한다.

## Decision

v2 task 는 그래프 단위로 제출한다(`agent.task_graph_submit`, 검증만 하는 `agent.task_graph_validate`). 순서는 세 단계다.

1. 그래프 전체를 검증한다. task 수(상한 `MAX_GRAPH_TASKS` = 1000), id, 참조, 타입, binding, 매핑, 조합 규칙, 순환을 확인하고 실패하면 아무것도 쓰지 않는다. 오류에는 제출 정의 안의 JSON Pointer 를 싣는다.
2. task 를 모두 Waiting 으로 쓴다. 쓰는 도중 실패하면 쓴 task 를 지운다.
3. 그래프 레코드(`tasty.agent.task_graph.<그래프 id>`, `tasty.task_graph/v1`) 하나를 쓰고 readiness 를 평가한다.

`graph_id` 를 가진 task 는 그 그래프 레코드가 없으면 readiness 평가에서 항상 대기한다. 그래서 2단계 중에 러너 tick 이 돌아도 실행되지 않고, 활성화는 레코드 한 키의 쓰기로 정해진다. 그래프의 task 가 모두 지워지면 레코드도 지운다. 운영 규칙은 [작업 러너 §그래프 제출](../dev-guide/agent-runner.md#그래프-제출).

## Consequences

- 검증 실패는 아무것도 쓰지 않는다. 2단계(task 기록)와 그래프 레코드 쓰기의 저장 실패는 쓴 task 를 지워 남는 task 가 없다. 러너 tick 이 끼어도 활성화 전 task 는 실행되지 않으므로, 활성화의 정확성은 잠금에 기대지 않는다.
- 3단계(readiness 반영)는 레코드를 쓴 뒤라 롤백하지 않는다. 이 단계의 저장 실패는 오류(`AgentError::GraphPartiallyActivated`, IPC `-32603`)로 돌려주고 `error.data` 에 `graph_id` 와 `possibly_active: true` 를 싣는다. 그래프는 활성이며 이미 Ready 가 된 task 는 러너가 실행한다. 같은 id 로 다시 제출하면 거절된다.
- 3단계 도중 호스트가 죽어도 같다. 반영하지 못한 의존 없는 task 는 Waiting 으로 남는다. 결정 당시에는 재시작이 readiness 를 다시 평가하지 않아 수동 복구가 필요했다. 2026-10-07 부터 부팅·러너 시작이 Waiting task 를 다시 평가해 남은 task 를 활성화한다([ADR-0071](0071-typed-task-completion-is-one-write-per-attempt.md) 과 같은 변경). 다시 제출하는 절차는 [작업 러너 §한계](../dev-guide/agent-runner.md#한계).
- 2단계에서 롤백까지 실패하면 활성화되지 않은 task 가 남는다. 실행되지 않으며 삭제·purge 로 지운다.
- readiness 평가는 task 마다 그래프 레코드 존재를 확인해야 한다(`TaskStore::readiness_graph`).
- 제출 전체가 memory 잠금 안에서, 앱의 IPC 처리 경로(main 스레드의 IPC 큐) 위에서 돈다. 그동안 memory 를 쓰는 러너 tick 과 같은 경로의 다른 IPC 요청 전체가 기다린다. memory 를 쓰지 않는 `system.ping` 도 제출이 끝날 때까지 기다렸다(아래 실측). 처음 구현은 3단계가 task 마다 workspace 목록을 저장소에서 다시 읽어 1000 개 제출이 5.7s 걸렸고, 그동안 다른 연결의 `memory.get` 이 5369ms 를 기다렸다(호스트 IPC 실측). 그래서 상한을 200 으로 두었다.
- 지금은 3단계와 하류 전파(`TaskStore::settle_waiting`)가 저장소 목록을 한 번 읽고 작업본을 갱신한다. 메모리 안의 readiness 그래프 재구성은 task 마다 남아 있어 비용은 여전히 제곱이지만 상수가 작다. `TaskStore::submit_graph` 를 같은 머신에서 번갈아 잰 값(dev 프로필, 3회 최소~최대)은 이전 구현이 200 개 178~539ms · 1000 개 3781~5441ms, 지금 구현이 200 개 38~85ms · 1000 개 181~488ms 다. 지금 구현의 1000 개가 이전 구현의 200 개와 같은 범위라 상한을 1000 으로 둔다. 초과는 `-32602`(`location: /tasks`)로 거절한다.
- 호스트 IPC 실측(격리 GUI 인스턴스, dev 빌드, 러너 정지, 제출 동안 다른 연결이 10ms 간격으로 `system.ping`, 측정 전 ping 기준선 0.09~0.10s):

  | 형태 | 제출 시간 | 제출 중 ping 최대 |
  |---|---|---|
  | 독립 1000(기존 task 1000 개가 있는 workspace, 1회) | 0.356s | 0.258s |
  | 독립 1000(새 workspace, 2회) | 1.515s / 0.390s | 1.404s / 0.285s |
  | 사슬 1000(새 workspace, 2회) | 0.354s / 0.391s | 0.247s / 0.285s |

  상한 크기의 그래프를 제출하면 그동안 호스트의 다른 IPC 가 최악 약 1.4s 멈출 수 있다. 이 지연을 받아들이고 상한을 1000 으로 유지한다. release 빌드는 재지 않았다.

## Alternatives Considered

- **task 를 하나씩 만들고 앞 task 를 예약 상태로 두기**: v1 의 `reserved_for_fallback` 과 같은 방식이다. 예약을 푸는 쓰기가 task 마다 필요해 활성화가 원자적이지 않다.
- **러너를 멈추거나 저장소 잠금을 잡고 쓰기**: 러너가 workspace 별 스레드라 모든 경로가 같은 잠금을 따라야 하고, 잠금을 쥔 채 실패하면 러너가 멈춘다.
- **그래프 전체를 레코드 하나로 저장하기**: 기존 task 단위 조회·삭제·재시도·DAG 화면이 모두 task 키를 읽으므로 그래프 안의 task 를 따로 다루는 경로가 생긴다.

## Reconsideration Triggers

- 메모리 저장소가 여러 키 쓰기를 한 트랜잭션으로 제공하면 2·3단계를 하나로 합친다(`tasty_memory` 의 쓰기 API).
- 재시작 시 readiness 를 다시 평가하는 경로가 생기면 Consequences 의 남는 Waiting 을 지운다(`TaskStore` 의 복구 경로).
- readiness 그래프를 task 마다 다시 만들지 않도록 바꾸면 제출 시간을 다시 재고 `MAX_GRAPH_TASKS` 를 조정한다(`TaskStore::settle_waiting`).

## References

- [ADR-0068](0068-typed-task-contracts-live-in-a-separate-record-namespace.md) — v2 task 의 저장 namespace 와 envelope.
- [ADR-0042](0042-agent-coordination-and-task-views.md) — 작업 조율과 DAG 화면.
- `crates/tasty-agent/src/task/store/graph_submit.rs`
