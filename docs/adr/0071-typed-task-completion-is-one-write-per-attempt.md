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
- 끝난 회차에 같은 지문의 보고가 오면 같은 레코드를 돌려주고 하류 반영만 다시 적용한다. 다른 지문이면 거절한다. 지문은 보고 원문이 아니라 결과와 종결 종류의 해시다.
- 거절은 `AgentError::CompletionRejected` 이고 IPC 응답은 사유와 두 회차 id 를 싣는다. 코드는 [API 규약](../dev-guide/api-conventions.md)의 도메인 오류 코드 표에 있다.
- `RunnerLoop` 는 저장소 오류로 기록하지 못한 보고를 보관하고 다음 tick 에 같은 보고를 다시 낸다. 그동안 다시 poll 하지 않고 handle 과 permit 을 유지한다. 보류에는 횟수·시간 상한을 두지 않는다. 보류가 풀리는 조건은 가이드에 있다.
- 훅 대기와 저장한 실행 handle 은 dispatch 한 회차 id 를 저장한다. 재시작 복구는 handle 의 회차로 보고하므로 재시도 뒤 남은 옛 handle 이 새 회차를 끝내지 않는다. snapshot 의 `sources` 는 원본 회차 id(`producer_attempt`)를 기록한다.
- v1 task 에는 회차를 두지 않는다. v1 은 결과 쓰기와 상태 전이의 두 번 쓰기를 유지하되, 전이가 맞지 않는 보고는 결과를 쓰기 전에 거절한다. v1 은 두 쓰기 사이에서 실패하면 결과만 남은 Running task 가 될 수 있다. v1 출력은 계약 검증이 없어 두 쓰기 사이에 공개되는 미확정 결과가 없고, 기존 호출자의 응답 형식을 바꾸지 않으려고 이 차이를 남긴다.
- 명시 `retry` 는 같은 task 레코드에 새 회차를 연다. 이미 실행된 fallback 의 결과와 전파는 되돌리지 않는다. 그래서 fallback 이 Ready·Running·Succeeded 인 v2 task 의 `retry` 는 거절한다. 본 작업이 다시 성공하면 `one_of` 소비자가 성공한 원본 둘을 보게 되기 때문이다(실측: 재시도 뒤 소비자의 입력 해석이 "one_of expects exactly one succeeded source" 로 실패했다).

현재 동작은 [agent runner 가이드](../dev-guide/agent-runner.md)의 "실행 회차와 완료" 절에 있다.

### agent task 회차의 결과

`agent` command(Claude·Codex 세션에 지시 하나를 보내는 task)도 위 회차와 한 번의 완료 쓰기를 따른다. 회차의 결과를 무엇으로 정하는지는 다음과 같이 정한다.

- 회차의 끝은 provider 플러그인이 훅에서 보고한 턴의 끝이다. 세션이 idle 이 된 것만으로는 끝내지 않는다. 사용자의 턴과 늦게 도착한 앞 턴의 끝도 idle 로 보이기 때문이다. 턴 보고는 그 보고 하나만 여는 별도 플러그인 권한으로 받아, provider 플러그인이 보고하려고 다른 `agent.*` 를 얻지 않게 한다.
- 호스트가 surface 마다 회차 하나를 (workspace, task) 로 묶어 보고를 귀속한다. 이 묶음은 영속하지 않는다. 재시작 뒤에는 보고를 어느 회차에 귀속할지 알 수 없으므로 실행 중이던 회차는 실패로 끝낸다.
- 이미 열린 세션에는 idle 이고 다른 회차가 묶지 않았을 때만 지시를 보낸다. 사용자의 턴과 섞지 않기 위해서다. 지시 끝에 회차 표지를 붙이고, 그 표지가 실린 프롬프트로 시작한 턴만 이 회차의 턴으로 받는다. 묶은 뒤 지시가 전달되기 전에 사용자가 시작한 턴을 가리기 위해서다. 훅이 프롬프트를 넘기지 않으면 표지를 볼 수 없어 시작 보고를 그대로 받는다.
- 기본 출력은 턴의 최종 답변이다. 다른 출력 타입은 같은 회차에 명시 제출한 값이 결과다. 제출은 도착할 때 출력 타입으로 검사하고, 턴이 끝날 때 위 완료 쓰기로 확정한다. 같은 회차의 다른 값은 거절한다. 제출 없이 끝난 턴은 실패로 끝내고 진단을 위해 마지막 답의 앞부분을 실패 기록에 남긴다.
- 회차 id 는 짐작할 수 있으므로 회차마다 예측할 수 없는 토큰을 만들어 지시에 싣고, 제출은 그 토큰을 함께 내야 받는다. **이 토큰은 보안 경계가 아니다.** 다른 회차나 다른 호출자가 실수로 낸 값을 막는 데만 쓴다. 로컬 IPC 는 신뢰 경계로 둔다([ADR-0011](0011-secrets-and-local-trust.md)). 같은 컴퓨터의 호출자는 세션 화면이나 실행 기록에서 토큰을 읽을 수 있다.
- 회차 결과 쓰기(완료 보고·제출)의 거절은 다른 도메인이 다른 뜻으로 쓰는 오류 코드와 겹치지 않는 코드로 낸다. 코드는 [API 규약](../dev-guide/api-conventions.md)의 도메인 오류 코드 표에 있다.
- task 는 세션을 닫지 않는다. 사용자가 결과를 확인할 수 있게 둔다. 예외로, task 가 spawn 응답을 기다리다 포기한 뒤에 뜬 새 세션은 task 가 닫는다. 이 task 가 만들었지만 어느 회차에도 묶이지 않아 아무도 쓰지 않는 세션이기 때문이다. 기존 세션은 닫지 않는다.

현재 동작과 상한·실패 코드는 [agent runner 가이드](../dev-guide/agent-runner.md)의 "agent task" 절에 있다.

## Consequences

- 쓰기가 실패하면 상태·결과·하류가 모두 그대로다. 성공 알림이 실패한 저장보다 먼저 나가지 않는다.
- 같은 보고의 재전송이 안전하다. 쓰기 뒤 하류 반영이 실패한 보고도 같은 보고를 다시 내면 마무리된다.
- 하류 반영(`propagate_transition`)은 여전히 task 마다 따로 쓴다. 반영 도중 실패하면 일부 하류만 옮겨진 상태가 남는다. 같은 보고를 다시 내면 이어서 옮긴다. 다시 낼 보고가 사라지는 재시작에 대비해 부팅·러너 시작이 Waiting task 전체를 다시 평가한다(`TaskStore::resettle_waiting`). 이 평가는 task 수에 비례해 한 번 목록을 읽고 task 마다 메모리 안 그래프를 다시 만든다.
- 회차 id 를 생략한 외부 보고는 지금 회차에 적용된다. 외부 도구가 회차를 모르면 재실행 사이의 늦은 보고를 막지 못한다. CLI·IPC 에 회차 id 를 받는 자리를 둬서 호출자가 고를 수 있게 했다.
- 저장소가 계속 실패하면 보류된 task 의 permit 이 그동안 묶인다. 묶이는 수는 보류된 task 수와 같다. 상한을 두고 포기하면 결과 없이 Running 인 task 의 자원을 다른 task 에 넘기게 되므로, 자원을 오래 쥐는 쪽을 택했다.
- 지문은 보고 원문이 아니라 해시다. 충돌하면 다른 보고를 같은 보고로 볼 수 있으나 64비트에서 실제로 문제가 될 확률은 무시한다.
- agent task: 에이전트의 답이나 구조화 판정을 binding·전이 조건으로 바로 쓸 수 있다. 사용자가 쓰는 세션을 넘겨도 사용자의 턴과 섞이지 않는 대신, 사용자가 세션을 계속 쓰면 task 는 기다린다(task 기한이 상한이다).
- agent task: 턴 보고를 구현하지 않은 provider 는 agent task 로 쓸 수 없다. 결과를 내지 않고 턴을 끝낸 에이전트에게 다시 묻지 않으며, 재시작하면 실행 중이던 agent task 는 실패하고 세션에 남은 답은 결과가 되지 않는다.
- 종결 이벤트(`agent.task_finished`)는 아직 회차 id 를 싣지 않는다. 페이로드는 번들 플러그인이 공유하는 프로토콜 크레이트에 있어 바꾸면 모든 번들 플러그인의 버전을 올려야 한다. 구독자는 `agent.task_get` 으로 회차를 읽는다.

## Alternatives Considered

- **두 메서드를 유지하고 러너가 실패 시 멈추게 한다.** 상태 전이가 실패했을 때 이미 쓴 결과를 되돌릴 수 없다. 외부 보고·훅 경로마다 같은 처리를 반복해야 한다.
- **memory 의 CAS(`PutOpts::cas`)로 레코드 버전을 맞춘다.** 동시 보고를 막는 데는 맞지만 같은 보고의 재전송을 알아보지 못한다. 완료 경로는 이미 memory 잠금 하나 안에서 읽고 써서 버전 경합이 생기지 않는다.
- **완료 보고 원문을 회차에 그대로 저장한다.** 출력이 큰 task 는 레코드가 두 배가 된다. 비교에는 지문으로 충분하다.
- **재시도마다 새 후속 실행 범위를 연다.** 회차별로 하류 task 를 따로 두면 fallback 결과를 그대로 둔 채 본 작업을 다시 돌릴 수 있다. 하류가 task 레코드 하나라 회차별 범위를 표현할 수 없고, 경로(route) 설계가 생긴 뒤에 다시 본다. 그때까지는 거절하고 새 task 제출을 안내한다.
- **agent task: idle 전이를 턴 끝으로 본다.** 사용자의 턴과 늦게 온 앞 턴의 끝을 구별할 수 없다.
- **agent task: 화면 출력에서 답을 읽는다.** 렌더링·줄바꿈에 따라 값이 바뀌고 구조화 값을 검증할 수 없다.
- **agent task: 최종 답변에서 JSON 을 파싱해 구조화 출력으로 쓴다.** 답변 형식이 어긋나면 업무 판단 없이 실패하고, 어느 부분이 결과인지 정할 규칙이 생긴다. 명시 제출은 도착 즉시 검사해 에이전트가 같은 턴에서 고칠 수 있다.
- **agent task: 턴 묶음을 저장소에 영속한다.** 재시작 사이에 온 보고와 provider 세션의 상태를 맞출 수단이 없어, 영속해도 귀속을 보장할 수 없다.
- **agent task: 지시를 보낸(tell 이 응답한) 뒤의 시작 보고만 받는다.** 지시의 시작 보고가 tell 응답보다 먼저 도착할 수 있어 자기 턴을 놓친다. 시각이나 전달 순서로는 사용자 턴을 가를 수 없어 프롬프트의 표지로 가린다.
- **agent task: 회차 id 만 맞으면 제출을 받는다.** 회차 id 는 `<task id>#<번호>` 라 짐작할 수 있어 같은 그래프를 돌리는 다른 호출자의 실수를 막지 못한다.
- **agent task: 토큰을 보안 경계로 삼는다.** 같은 컴퓨터의 호출자는 세션 화면과 실행 기록을 읽을 수 있어 비밀로 지킬 수 없다. 로컬 IPC 신뢰 경계를 바꾸는 일은 이 결정의 범위가 아니다.
- **validating·persisting 같은 중간 phase 를 상태로 둔다.** 완료가 한 번의 쓰기라 바깥에서 관측할 수 없는 상태다. 후처리 단계가 생기면 그때 phase 를 정한다.

## Reconsideration Triggers

코드와 설정에서 확인:

- 후처리(postprocess) 단계나 사람 입력 대기처럼 완료가 두 번 이상의 외부 작업으로 나뉘면 phase 와 회차 상태를 다시 정한다. `TaskStore::complete` 가 두 번 이상 `put` 하게 되면 이 결정의 전제가 깨진 것이다.
- 하류 반영을 한 트랜잭션으로 묶을 수 있는 저장소 API(여러 키 원자 쓰기)가 생기면 `propagate_transition` 의 부분 반영 한계를 다시 본다.
- 종결 이벤트 페이로드를 바꿀 다른 이유가 생기면 회차 id 와 레코드 버전을 함께 싣는다.
- agent task: claude·codex 외의 provider 가 턴 경계를 보고하게 되면 provider 목록을 넓힌다. provider 가 턴 id 를 지시와 함께 돌려주게 되면 시작 보고와 표지 대신 턴 id 로 귀속한다.
- agent task: 턴 보고를 하는 provider 훅이 프롬프트를 넘기지 못하게 되면(`prompt_seen` 이 없는 시작 보고) 기존 세션의 사용자 턴을 가릴 수 없다. 그 provider 의 귀속 방식을 다시 정한다.
- agent task: 로컬 IPC 밖(원격 attach 등)의 호출자가 제출할 수 있게 되면 회차 토큰을 보안 경계로 다룰지 다시 정한다.

실행 결과로 확인:

- 저장소 장애 동안 permit 이 묶여 다른 task 가 오래 기다린 사례가 관측되면 보류 상한과 그때의 task 처리(Unknown 상태 등)를 정한다. 러너 로그의 "completion not recorded, retrying next tick" 경고가 반복되는지로 확인한다.

- 외부 보고자가 회차 id 없이 보낸 보고가 재실행된 task 를 끝낸 사례가 관측되면 v2 외부 보고에 회차 id 를 필수로 바꾼다.
- agent task 가 `result_missing` 으로 자주 끝나 재지시가 필요하면 추가 턴 대기를 검토한다. task 결과의 `typed_result.error.code` 분포로 확인한다.

## References

- `crates/tasty-agent/src/task/attempt.rs` — 회차, `Completion`, 지문.
- `crates/tasty-agent/src/task/store/complete.rs` — `TaskStore::complete`.
- `crates/tasty-agent/src/runner.rs` — `RunnerLoop::pending`, `completion_retryable`.
- `crates/tasty-task-runtime/src/runner_host/attempt_record.rs` — `RunnerContext::complete_task`, 저장 handle 의 회차(`attempt_id` 키).
- `crates/tasty-task-runtime/src/runner_thread.rs` — 재시작 복구 보고(`mark_dead_tasks`·`finalize_precise_tasks`).
- `crates/tasty-task-runtime/src/agent_turns.rs` — agent task 턴 표(회차 묶음·표지 대조·제출).
- `crates/tasty-task-runtime/src/runner_host/agent.rs` — agent task dispatch(회차 토큰·표지·늦은 세션 닫기).
- [ADR-0011](0011-secrets-and-local-trust.md) — 로컬 IPC 신뢰 경계.
- [ADR-0068](0068-typed-task-contracts-live-in-a-separate-record-namespace.md) — v2 레코드 형식.
- [ADR-0069](0069-typed-task-graphs-activate-through-a-graph-record.md) — 그래프 활성화와 readiness.
