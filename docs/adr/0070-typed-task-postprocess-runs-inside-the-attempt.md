# ADR-0070: v2 task 의 후처리 CLI 는 같은 실행 회차 안에서 실행하고 결과가 불명이면 다시 실행하지 않는다

- **Status**: Accepted
- **Date**: 2026-10-07
- **Tags**: agents, tasks, types, postprocess, idempotency
- **Group**: agents

## Context

판정·요약처럼 본 작업의 결과를 다른 CLI(모델을 부르는 도구 등)로 한 번 더 처리해 타입이 있는 출력을 얻어야 하는 task 가 있다. 이 처리를 별도 task 로 두면 본 작업의 원본 결과를 binding 으로 넘겨야 하고, 실패했을 때 본 작업까지 다시 실행하는지·permit 을 언제 놓는지·하류가 언제 움직이는지가 두 task 의 상태로 흩어진다. 외부 CLI 는 비용이 들고 부작용이 있을 수 있어 같은 입력으로 두 번 실행되는 것도 피해야 한다.

필요한 조건은 다음과 같다.

- 본 작업이 성공한 뒤에만 실행하고, 재시도해도 본 작업은 다시 실행하지 않는다.
- 후처리가 끝나기 전에는 하류가 움직이지 않고 세마포어·lease 를 다른 task 에 넘기지 않는다.
- 재시도가 남은 실패로 `on_failure`·하류 건너뛰기가 일어나지 않는다.
- 호스트가 재시작했을 때 이미 시작한 실행을 모르는 사이에 다시 실행하지 않는다.
- 특정 CLI 를 위한 코드를 호스트에 두지 않는다.

## Decision

후처리는 계약의 선택 슬롯(`TaskContract.postprocess`)이고 run·custom task 에만 둔다. 본 작업과 후처리는 같은 실행 회차(ADR-0069)에 속한다.

- 본 작업의 성공 보고는 task 를 끝내지 않는다. 저장소는 회차에 본 작업 결과와 후처리 진행(`Pending` → `Started` → `Finished`)을 기록하고 task 를 Running 으로 둔다. 본 작업이 실패하면 후처리 없이 끝낸다.
- 호스트는 `Started` 를 기록한 뒤에만 프로세스를 띄운다. 실행 보고는 회차 id 와 실행 번호를 지니며, 호스트는 보고를 회차·번호와 함께 별도 키에 저장한다.
- 재시도가 남은 실패 보고는 다음 실행을 `Pending` 으로 예약하고 task 를 Running 으로 둔다. 마지막 보고에서만 결과를 확정하고 종결하므로 `on_failure` 와 하류 반영은 한 번 일어난다. permit 은 그 종결 때 놓는다.
- 재시작 때 `Started` 인데 저장된 보고가 없으면 `outcome_unknown` 실패로 끝내고 재시도 예산이 남아도 다시 실행하지 않는다. 살아 있는 PID 는 같은 프로세스라는 증거가 아니므로 쓰지 않는다. 저장된 보고가 있으면 그것으로 확정하고, `Pending` 이면 예약대로 실행한다.
- 취소·시간 초과는 그 실행이 만든 프로세스 그룹(Windows 는 job)만 종료하고, 종료를 확인한 뒤에 permit 을 놓는다.
- 같은 입력으로 다시 실행해 결과가 바뀔 근거가 없는 실패(`output_validation`·`stdin_mapping`), 사용자의 중단 의도를 거스르는 실패(`cancelled`), 다시 실행하면 같은 후처리가 두 번 실행될 수 있는 실패(`outcome_unknown`)는 재시도하지 않는다. 나머지 실패는 CLI 의 일시적 상태로 달라질 수 있어 예산 안에서 다시 실행한다. 원인별 표와 이유는 가이드에 있다.
- 러너가 멈추면(workspace 정리·앱 종료) 진행 중인 후처리를 `cancelled` 로 중단하고 그 보고를 저장한다. 재시작 뒤에는 그 보고로 실패가 확정되며 다시 실행하지 않는다. 멈춘 시점에 프로세스를 끝냈으므로 결과가 불명인 것이 아니라 중단된 것이고, 재시작 뒤 자동으로 다시 실행하면 사용자가 모르는 사이에 외부 CLI 가 다시 불린다. 보고를 저장하기 전에 호스트가 끝나면 `outcome_unknown` 이 된다.
- 정상 종료 때는 그룹 종료와 보고 저장을 짧은 상한 안에서 기다린다. 기다리지 않으면 정지 표지를 세운 직후 프로세스가 끝나 그룹이 남고 재시작 뒤 `cancelled` 대신 `outcome_unknown` 이 된다. 상한을 두는 것은 멈춘 runner 가 앱 종료를 막지 않게 하기 위해서다. 값은 가이드에 있다.
- 비정상 종료(SIGTERM 등)에는 Tasty 가 개입할 수 없다. Linux 는 후처리를 `tasty_reaper::spawn_bound_to_host` 로 띄워 그룹 리더가 PDEATHSIG 로 SIGTERM 을 받게 한다. 리더가 직접 실행한 CLI 이면 그것으로 끝나지만, 그룹의 다른 프로세스는 신호를 받지 않아 리더가 전달하지 않으면 남는다. Windows 는 실행별 KILL_ON_JOB_CLOSE job 이 호스트 종료와 함께 닫혀 job 안의 프로세스가 끝난다. macOS 는 묶지 않는다. 그룹 전체를 끝내려면 Tasty 에 SIGTERM 처리기가 필요하고 그것은 후처리만의 문제가 아니다(기존 run 도 같다).
- 완료 보고 밖에서 task 가 끝나면(취소 등) `attempt.postprocess.phase` 를 `finished` 로 닫는다. `run` 은 마지막으로 시작한 실행 번호다(없으면 0). 끝난 task 의 기록을 읽는 쪽이 진행 중으로 보지 않게 한다.
- stdout 수집·stderr 보존·실행 시간·재시도 횟수와 대기에 유한한 상한을 두고, 계약 검사가 범위 밖 값을 생성 때 거절한다. stdout 은 상한을 넘으면 잘린 값으로 성공시키지 않고 실패한다. 값과 각 상한의 근거는 가이드에 있다.
- 후처리 자식은 Tasty 프로세스의 환경을 받되, 바깥 Claude Code 세션이 남긴 표지·비밀과 바깥 Tasty 인스턴스의 신원 변수는 지운다. 남기면 후처리가 부르는 Claude Code 가 자신을 바깥 세션의 자식으로 오인하고 세션 비밀이 무관한 프로세스로 새며, 후처리 CLI 가 부른 `tasty` 가 다른 인스턴스의 신원으로 요청한다. 터미널 셸과 달리 후처리에는 자기 값으로 덮어쓸 surface 가 없다. 사용자가 넣는 설정·인증과 그 밖의 `TASTY_*` 는 그대로 넘기며, Tasty 가 task 별 변수를 더하지는 않는다. 목록은 가이드의 "runner 자식의 환경" 절에 있다.

계약 형식, 상한, stdout 해석, 실패 원인 목록은 [agent runner 가이드](../dev-guide/agent-runner.md)의 "후처리 CLI" 절에 있다.

### Run·custom 자식으로 범위 확장

위 환경 결정(바깥 Claude Code 세션 표지·비밀과 바깥 Tasty 인스턴스의 신원 변수 네 개를 지우고 나머지는 넘긴다)은 후처리 CLI 만이 아니라 Tasty 가 작업 실행을 위해 띄우는 모든 자식에 적용한다: `Run` task 의 명령과 reduce 의 custom 전략 셸(`run_custom_shell`, runner 의 reduce task 와 `agent.task_reduce`). 같은 이유다. 어느 자식이든 바깥 세션의 값을 받으면 자식이 부른 `claude`·`tasty` 가 다른 세션·인스턴스의 신원으로 동작한다. 규칙은 한 곳(`tasty_agent::child_env`)에 두어 세 경로가 같이 바뀐다.

자식에 이 인스턴스의 신원을 넣지는 않는다. 터미널 셸은 자기 surface 의 `TASTY_SURFACE_ID`·`TASTY_PARENT_HOME` 을 넣지만, 작업의 자식은 터미널 surface 가 아니어서 넣을 surface id 가 없고, 자식이 띄운 에이전트의 완료 알림은 그 알림을 기다리는 호출자의 것이라 `TASTY_PARENT_HOME` 을 이 인스턴스로 정할 근거가 없다.

### 예산을 다 쓴 후처리 실패

재시도 예산을 다 쓴 후처리 실패는 그 task 의 실패로 끝내고, 후처리만 다시 실행하는 수동 재시도는 두지 않는다. 그 갈래는 일반 실패와 같이 실패 정책(하류 건너뛰기·`on_failure` fallback)을 따른다. 그래프 전체의 결과는 DAG 요약이 나눠 보인다. 진행할 작업이 남은 동안은 진행 상태이고, 끝났을 때 실패와 성공이 섞였으면 `partially_failed` 로 실패만 있는 그래프와 구별한다. 판단 순서는 [agent-collaboration](../features/agent-collaboration/index.md)에 있다.

## Consequences

- 하류·permit·`on_failure` 의 시점이 task 하나의 종결로 정해진다. 본 작업 결과와 후처리 원본은 `raw` 에 따로 남는다.
- 재시도는 저장한 본 작업 결과로만 한다. 본 작업의 부작용이 반복되지 않는다.
- 결과가 불명인 실행은 자동으로 회복하지 않는다. 사용자가 확인하고 `retry` 해야 하며, 이때는 본 작업부터 새 회차로 실행한다.
- 후처리 보고와 stdout 은 task 레코드에 들어가므로 memory 값 상한 안에 있어야 한다. 그래서 stdout 수집과 stderr 보존에 상한을 두고, 재시도로 넘어간 실행은 원인·종료 코드만 남긴다.
- macOS 등 Linux 가 아닌 Unix 는 종료한 리더를 회수한 뒤에는 그룹 id 재사용을 막을 수 없어, 직접 자식이 끝난 뒤 파이프를 쥔 자손은 그룹 종료 대상에서 빠진다.

## Alternatives Considered

- **후처리를 별도 task 로 표현**: 하류·permit·실패 처리가 두 task 에 나뉘고 재시도 때 본 작업 결과를 다시 binding 해야 한다. 판정 task 만 실패해도 앞 task 는 이미 성공으로 하류에 알려진다.
- **본 작업 handle 안에서 후처리까지 실행(러너가 모르게)**: 재시도 대기·재시작 복원·취소를 실행기마다 구현해야 하고, `Started` 기록 없이 띄우면 재시작 뒤 중복 실행을 막을 수 없다.
- **재시작 뒤 결과 불명이면 재시도 예산 안에서 다시 실행**: 외부 CLI 가 이미 비용을 쓰거나 부작용을 남겼을 수 있다. 조용한 중복 실행보다 명시적 실패를 택했다.
- **재시도마다 본 작업부터 다시 실행**: 본 작업의 부작용이 반복되고, 판정만 흔들리는 경우에도 비용이 커진다.
- **예산을 다 쓴 뒤 후처리만 수동으로 다시 실행**: 끝난 task 를 회차 안으로 되돌리는 별도 전이와, 이미 건너뛴 하류·실행한 fallback 을 되돌리는 처리가 필요하다. 실패를 그 task 에서 끝내고 그래프 결과를 부분 오류로 구분해 보이는 쪽을 택했다. 다시 실행하려면 `retry` 로 본 작업부터 새 회차를 연다.

## Reconsideration Triggers

- 후처리 CLI 가 실행마다 멱등 키를 받아 중복 실행을 스스로 막을 수 있게 되면 결과 불명을 재시도 대상으로 다시 검토한다.
- memory 값 상한이 바뀌거나 결과를 artifact 로 따로 저장하게 되면 stdout 상한을 다시 정한다(`MAX_POSTPROCESS_STDOUT_BYTES`).
- Tasty 에 SIGTERM 처리기가 생기면 비정상 종료 때 그룹 전체를 끝내는 경로로 바꾼다.
- 후처리 단계를 두 개 이상 잇는 요구가 생기면 진행 기록을 단계 목록으로 넓히는 것을 검토한다.
- 본 작업의 부작용이 커서 후처리만 다시 실행해야 하는 사용 사례가 반복되면 후처리 전용 수동 재시도를 다시 검토한다.

## References

- [agent runner 가이드](../dev-guide/agent-runner.md) — 후처리 CLI 절
- [ADR-0069](0069-typed-task-completion-is-one-write-per-attempt.md) — 실행 회차와 한 번의 완료 쓰기
- 구현: `crates/tasty-agent/src/child_env.rs`(작업 자식의 환경), `crates/tasty-agent/src/task/postprocess.rs`, `crates/tasty-agent/src/task/store/postprocess.rs`, `crates/tasty-task-runtime/src/runner_host/postprocess.rs`
