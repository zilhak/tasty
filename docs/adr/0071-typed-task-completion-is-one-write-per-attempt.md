# ADR-0071: v2 task 완료는 실행 회차마다 레코드 한 번의 쓰기로 확정한다

- **Status**: Accepted
- **Date**: 2026-10-07
- **Tags**: agents, tasks, types, idempotency, storage
- **Group**: agents

## Context

v2 task 의 완료는 결과 쓰기(`set_result`)와 상태 전이(`set_state`)라는 두 번의 저장으로 이뤄졌다. 러너·재시작 복구·훅 완료·훅 만료·IPC `task_set_result` 가 각자 두 메서드를 불렀다. 이 구조에는 다음 문제가 있었다.

- 결과 쓰기가 실패해도 러너는 상태 전이를 이어서 불렀다. 결과 없이 종결되거나 하류가 움직일 수 있었다.
- 상태 전이가 실패하면 결과만 남은 Running task 가 됐고, 러너는 이미 handle 과 permit 을 놓았다.
- 같은 보고를 다시 보내면 결과를 덮어썼다. 같은 보고인지 다른 보고인지 구별하지 않았다.
- 훅 대기는 task id 만 알았다. task 가 끝나고 다시 실행된 뒤 옛 훅이 오면 새 실행을 끝낼 수 있었다.
- binding snapshot 은 원본의 시작·종결 시각으로 어느 실행의 값인지 표시했다. 같은 ms 에 다시 실행되면 구별하지 못한다.

## Decision

v2 task 는 Ready → Running 전이마다 새 실행 회차를 받는다(`Task.attempt`, id `<task id>#<번호>`). 모든 완료 경로는 `Completion`(회차 id·결과·성공/실패)을 만들어 `TaskStore::complete` 하나로 기록한다.

- v2 는 결과 확정(출력 검증 포함), 종결 상태, 회차의 완료 지문을 레코드 한 번의 `put` 으로 쓴다. 이 쓰기가 끝난 뒤에만 하류 readiness·fallback 을 평가하고 대기자에게 알린다.
- 보고한 회차가 지금 회차와 다르면 적용하지 않는다. 회차 id 를 생략한 보고는 지금 회차의 보고로 본다.
- 끝난 회차에 같은 지문의 보고가 오면 같은 레코드를 돌려주고 하류 반영만 다시 적용한다. 다른 지문이면 거절한다. 지문은 결과와 종결 종류를 FNV-1a 64 로 해시한 값이다.
- 거절은 `AgentError::CompletionRejected` 이고 IPC 에서는 `-32014` 로 사유와 두 회차 id 를 싣는다.
- `RunnerLoop` 는 저장소 오류로 기록하지 못한 보고를 보관하고 다음 tick 에 같은 보고를 다시 낸다. 그동안 다시 poll 하지 않고 handle 과 permit 을 유지한다. 보류에는 횟수·시간 상한을 두지 않는다. 저장이 회복되거나, 러너를 다시 시작할 때 점유 정리가 회수하거나(보류 보고는 사라지고 task 는 Failed), task 가 밖에서 종결될 때 풀린다.
- 훅 대기와 저장한 실행 handle 은 dispatch 한 회차 id 를 저장한다. 재시작 복구는 handle 의 회차로 보고하므로 재시도 뒤 남은 옛 handle 이 새 회차를 끝내지 않는다. snapshot 의 `sources` 는 원본 회차 id(`producer_attempt`)를 기록한다.
- v1 task 에는 회차를 두지 않는다. v1 은 결과 쓰기와 상태 전이의 두 번 쓰기를 유지하되, 전이가 맞지 않는 보고는 결과를 쓰기 전에 거절한다. v1 은 두 쓰기 사이에서 실패하면 결과만 남은 Running task 가 될 수 있다. v1 출력은 계약 검증이 없어 두 쓰기 사이에 공개되는 미확정 결과가 없고, 기존 호출자의 응답 형식을 바꾸지 않으려고 이 차이를 남긴다.
- 명시 `retry` 는 같은 task 레코드에 새 회차를 연다. 이미 실행된 fallback 의 결과와 전파는 되돌리지 않는다. 그래서 fallback 이 Ready·Running·Succeeded 인 v2 task 의 `retry` 는 거절한다. 본 작업이 다시 성공하면 `one_of` 소비자가 성공한 원본 둘을 보게 되기 때문이다(실측: 재시도 뒤 소비자의 입력 해석이 "one_of expects exactly one succeeded source" 로 실패했다).

현재 동작은 [agent runner 가이드](../dev-guide/agent-runner.md)의 "실행 회차와 완료" 절에 있다.

## Consequences

- 쓰기가 실패하면 상태·결과·하류가 모두 그대로다. 성공 알림이 실패한 저장보다 먼저 나가지 않는다.
- 같은 보고의 재전송이 안전하다. 쓰기 뒤 하류 반영이 실패한 보고도 같은 보고를 다시 내면 마무리된다.
- 하류 반영(`propagate_transition`)은 여전히 task 마다 따로 쓴다. 반영 도중 실패하면 일부 하류만 옮겨진 상태가 남는다. 같은 보고를 다시 내면 이어서 옮긴다. 다시 낼 보고가 사라지는 재시작에 대비해 부팅·러너 시작이 Waiting task 전체를 다시 평가한다(`TaskStore::resettle_waiting`). 이 평가는 task 수에 비례해 한 번 목록을 읽고 task 마다 메모리 안 그래프를 다시 만든다.
- 회차 id 를 생략한 외부 보고는 지금 회차에 적용된다. 외부 도구가 회차를 모르면 재실행 사이의 늦은 보고를 막지 못한다. CLI·IPC 에 회차 id 를 받는 자리를 둬서 호출자가 고를 수 있게 했다.
- 저장소가 계속 실패하면 보류된 task 의 permit 이 그동안 묶인다. 묶이는 수는 보류된 task 수와 같다. 상한을 두고 포기하면 결과 없이 Running 인 task 의 자원을 다른 task 에 넘기게 되므로, 자원을 오래 쥐는 쪽을 택했다.
- 지문은 보고 원문이 아니라 해시다. 충돌하면 다른 보고를 같은 보고로 볼 수 있으나 64비트에서 실제로 문제가 될 확률은 무시한다.
- 종결 이벤트(`agent.task_finished`)는 아직 회차 id 를 싣지 않는다. 페이로드는 번들 플러그인이 공유하는 프로토콜 크레이트에 있어 바꾸면 모든 번들 플러그인의 버전을 올려야 한다. 구독자는 `agent.task_get` 으로 회차를 읽는다.

## Alternatives Considered

- **두 메서드를 유지하고 러너가 실패 시 멈추게 한다.** 상태 전이가 실패했을 때 이미 쓴 결과를 되돌릴 수 없다. 외부 보고·훅 경로마다 같은 처리를 반복해야 한다.
- **memory 의 CAS(`PutOpts::cas`)로 레코드 버전을 맞춘다.** 동시 보고를 막는 데는 맞지만 같은 보고의 재전송을 알아보지 못한다. 완료 경로는 이미 memory 잠금 하나 안에서 읽고 써서 버전 경합이 생기지 않는다.
- **완료 보고 원문을 회차에 그대로 저장한다.** 출력이 큰 task 는 레코드가 두 배가 된다. 비교에는 지문으로 충분하다.
- **재시도마다 새 후속 실행 범위를 연다.** 회차별로 하류 task 를 따로 두면 fallback 결과를 그대로 둔 채 본 작업을 다시 돌릴 수 있다. 하류가 task 레코드 하나라 회차별 범위를 표현할 수 없고, 경로(route) 설계가 생긴 뒤에 다시 본다. 그때까지는 거절하고 새 task 제출을 안내한다.
- **validating·persisting 같은 중간 phase 를 상태로 둔다.** 완료가 한 번의 쓰기라 바깥에서 관측할 수 없는 상태다. 후처리 단계가 생기면 그때 phase 를 정한다.

## Reconsideration Triggers

코드와 설정에서 확인:

- 후처리(postprocess) 단계나 사람 입력 대기처럼 완료가 두 번 이상의 외부 작업으로 나뉘면 phase 와 회차 상태를 다시 정한다. `TaskStore::complete` 가 두 번 이상 `put` 하게 되면 이 결정의 전제가 깨진 것이다.
- 하류 반영을 한 트랜잭션으로 묶을 수 있는 저장소 API(여러 키 원자 쓰기)가 생기면 `propagate_transition` 의 부분 반영 한계를 다시 본다.
- 종결 이벤트 페이로드를 바꿀 다른 이유가 생기면 회차 id 와 레코드 버전을 함께 싣는다.

실행 결과로 확인:

- 저장소 장애 동안 permit 이 묶여 다른 task 가 오래 기다린 사례가 관측되면 보류 상한과 그때의 task 처리(Unknown 상태 등)를 정한다. 러너 로그의 "completion not recorded, retrying next tick" 경고가 반복되는지로 확인한다.

- 외부 보고자가 회차 id 없이 보낸 보고가 재실행된 task 를 끝낸 사례가 관측되면 v2 외부 보고에 회차 id 를 필수로 바꾼다.

## References

- `crates/tasty-agent/src/task/attempt.rs` — 회차, `Completion`, 지문.
- `crates/tasty-agent/src/task/store/complete.rs` — `TaskStore::complete`.
- `crates/tasty-agent/src/runner.rs` — `RunnerLoop::pending`, `completion_retryable`.
- `crates/tasty-task-runtime/src/runner_host.rs` — `RunnerContext::complete_task`, handle 의 회차 저장(`persist_handle`).
- `crates/tasty-task-runtime/src/runner_thread.rs` — 재시작 복구 보고(`mark_dead_tasks`·`finalize_precise_tasks`).
- [ADR-0068](0068-typed-task-contracts-live-in-a-separate-record-namespace.md) — v2 레코드 형식.
- [ADR-0069](0069-typed-task-graphs-activate-through-a-graph-record.md) — 그래프 활성화와 readiness.
