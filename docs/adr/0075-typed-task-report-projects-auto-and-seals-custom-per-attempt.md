# ADR-0075: v2 task report 의 자동 항목은 조회 때 만들고 custom 기록은 회차별 키에 쓰다가 종결 전이로 닫는다

- **Status**: Accepted
- **Date**: 2026-10-08
- **Tags**: agents, tasks, types, dag, report, storage
- **Group**: agents

## Context

v2 그래프를 사람이 돌아볼 때 task 마다 무엇이 들어가 무엇이 나왔는지, 실행 중에 무엇을 했는지 한곳에서 읽을 수 있어야 한다. 결과 레코드에는 이미 상태·시각·입력 snapshot·출력·실패 단계가 있지만, 실행 중인 작업이 남기는 메모를 둘 자리가 없었다. 예약해 둔 `artifacts` 필드는 채우는 실행기가 없어 늘 비어 있었다.

조건:

- 메모는 실행 중인 회차에만 받고, 회차가 끝난 뒤의 쓰기가 결과를 바꾸면 안 된다.
- 메모가 커져도 task 상태 전이 쓰기가 실패하면 안 된다. memory 값 하나는 1 MiB 까지다.
- memory 저장소에는 여러 키를 한 번에 쓰는 트랜잭션이 없다. 저장소 잠금은 하나다.
- 셸 명령·후처리·reduce 셸·agent 세션이 각자 자기 회차에만 쓸 수 있어야 한다.
- report 는 기록이다. 뒤 task 의 입력이 되면 출력 타입 계약을 우회하는 통로가 된다.

## Decision

- **자동 항목은 살아 있는 원본이 있는 동안 복사하지 않는다.** `agent.dag_report` 가 조회할 때 task 레코드에서 종결 상태·시각·실제 입력·출력·실패·skip 과 종류별 값을 만든다(`tasty_agent::task::report::project_task`). 표준 출력·오류는 요청할 때만 싣는다.
- **retry 가 닫는 회차의 자동 항목은 한 번 굳혀 저장한다.** retry 는 다음 회차를 위해 레코드의 결과·입력을 지우므로, 지우기 직전에 그 회차의 투영(`raw` 포함)을 `tasty.agent.task_report_auto.<task id>.<n>` 에 쓴다. 원본이 사라진 뒤의 유일한 사본이므로 두 번째 원본이 아니다.
- **custom 기록은 회차마다 따로 저장한다.** 키는 `tasty.agent.task_report.<task id>.<n>` 이고 task 레코드와 분리한다. 항목은 텍스트(`seq`·`at`·`source`·`text`·`omit_by_limit?`)만 받는다.
- **회차 토큰으로 주소를 준다.** 러너가 dispatch 할 때 회차 토큰(16바이트 난수)을 task 레코드에 남기고, 자식에게 `TASTY_TASK_REPORT=<workspace>/<회차>/<source>/<토큰>/<task id>` 를, agent 에게는 지시문의 주소 한 줄을 준다. run 은 stderr 의 `::tasty-report::` 줄로도 쓴다.
- **닫힘은 종결 전이가 정한다.** append 는 저장소 잠금 안에서 task 레코드를 읽어, 토큰·회차가 마지막 dispatch 와 같고 상태가 Ready·Running 일 때만 쓴다. 종결 전이도 같은 잠금 안에서 레코드를 쓰므로, 그 쓰기 뒤의 append 는 모두 거절된다. 블록 자체에 닫힘 표시를 쓰는 별도 단계가 없다.
- **retry 는 이전 블록에 끝난 상태를 남긴다.** retry 가 레코드의 결과를 지우기 전에 그 회차 블록의 `settled` 에 상태를 적는다. `settled` 가 있는 블록에는 append 를 받지 않는다. retry 뒤 다음 dispatch 전에는 레코드가 Ready 이고 토큰도 그 회차 것이기 때문이다.
- **상한 초과는 실패가 아니다.** 한 건은 UTF-8 경계에서 자르고, 블록 합을 넘는 기록은 세기만 한다. 두 상한은 설정이며 `append < block` 이다.
- report 는 binding 이 읽는 출력 문서에 없으므로 입력으로 쓸 수 없다. `artifacts` 필드는 없앴다.

현재 형식과 경로별 사용법은 [작업 러너 §DAG report](../dev-guide/agent-runner.md#dag-report)에 있다.

## Consequences

- 지금 회차의 자동 항목은 레코드와 어긋날 수 없고 저장 비용이 없다. retry 로 닫힌 회차는 굳힌 값으로 입력·출력·실패를 계속 볼 수 있고, 그 값은 원본이 지워진 뒤에만 존재하므로 어긋날 짝이 없다. 이 저장이 생기기 전에 닫힌 회차의 자동 항목은 `null` 이다.
- report 키는 task 레코드 키보다 길다. task 제출은 가장 긴 report 키가 memory 키 길이 안에 드는 id 만 받는다.
- 메모 크기가 task 레코드 크기와 무관하므로 큰 report 가 상태 전이를 막지 않는다. 회차마다 키가 하나 늘어나며, task 를 지우거나 GC 할 때 1..마지막 회차의 키를 함께 지운다.
- 닫힘을 레코드 상태로 판정하므로 다중 키 트랜잭션 없이 "종결 뒤 쓰기 없음" 이 성립한다. Unknown 도 닫힌 회차로 본다.
- `source` 는 호출자가 밝힌 값이고, 토큰은 실수로 다른 회차에 쓰는 것을 막을 뿐 보안 경계가 아니다(로컬 IPC 가 신뢰 경계다). agent 세션은 세션 토큰으로 부르므로 `agent.report_append` 는 `agent` 권한으로 연다. 세션 호출은 `agent.task_submit_result` 처럼 자기 세션에 지시를 보낸 회차에만 쓰고, 플러그인 호출은 주소의 토큰이 쓸 블록을 정한다.
- run 의 표지 줄은 저장된 stderr 에도 남는다. stderr 를 그대로 보존한다는 기존 계약을 바꾸지 않으려는 선택이다.

## Alternatives Considered

- **자동 항목을 회차가 끝날 때마다 스냅샷으로 저장**: 레코드가 살아 있는 동안 같은 값을 두 번 저장하고 둘이 어긋날 수 있다. 기각하고, 원본이 지워지는 retry 때만 굳힌다.
- **이전 회차의 자동 항목을 내지 않기**: retry 한 task 를 돌아볼 때 실패한 회차의 입력·출력·실패 사유를 볼 수 없다. 기각.
- **custom 기록을 task 레코드 안에 두기**: 한 번의 쓰기로 닫힘을 맞출 수 있지만, 메모가 커지면 1 MiB 값 상한 때문에 상태 전이 쓰기가 실패한다. 기각.
- **블록에 닫힘 표시를 따로 쓰기**: 종결 쓰기와 블록 쓰기 사이에 호스트가 죽으면 열린 채 남는다. 트랜잭션 없이 원자성을 맞출 수 없다. 기각.
- **`artifacts` 를 report 자리로 쓰기**: 결과 타입 계약 안에 들어가 binding·후처리 stdin 이 읽을 수 있는 자리라 기록과 데이터의 경계가 흐려진다. 기각하고 필드를 없앴다.
- **stdout 표지 줄**: stdout 은 run 의 결과 자리라 표지가 출력 값에 섞인다. stderr 만 본다.

## Reconsideration Triggers

- task 레코드가 회차별 결과 이력을 갖게 되면 굳힌 자동 항목 대신 그 이력에서 투영할지 다시 본다.
- memory 저장소에 다중 키 트랜잭션이 생기면 블록에 닫힘을 직접 기록하는 쪽이 단순한지 다시 본다.
- report 를 뒤 task 가 읽어야 하는 요구가 생기면 binding 원본으로 열지, 출력 타입으로 옮기게 할지 다시 정한다.
- 로컬 IPC 에 호출자 인증이 생기면 `source` 를 호출자 신원으로 정할 수 있는지 다시 본다.

## References

- [작업 러너 §DAG report](../dev-guide/agent-runner.md#dag-report)
- [ADR-0069](0069-typed-task-completion-is-one-write-per-attempt.md) — 회차마다 결과와 종결을 한 번에 쓴다
- `crates/tasty-agent/src/task/report.rs`, `crates/tasty-agent/src/task/store/report.rs`, `crates/tasty-task-runtime/src/runner_host/report.rs`
