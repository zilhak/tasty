# ADR-0078: agent task 의 결과는 provider 가 보고한 턴의 끝에서 정하고 바쁜 세션에는 끼어들지 않는다

- **Status**: Accepted
- **Date**: 2026-10-07
- **Tags**: agents, tasks, types, claude, codex
- **Group**: agents

## Context

`custom` task 로 `claude.spawn`·`claude.tell` 을 부르면 자식이 idle 이 될 때까지 기다릴 수는 있지만, 결과는 IPC 응답(`child_surface_id` 등)이고 에이전트가 무엇을 답했는지는 task 결과에 없다. 리뷰 결과 같은 업무 값을 다음 task 의 입력이나 전이 조건으로 쓰려면 호출자가 화면을 읽어 다시 보내야 했다.

idle 은 턴이 끝났다는 신호로 부족하다. 사용자가 같은 세션에서 직접 대화한 턴도 idle 로 끝나고, 지시를 보내기 전에 끝난 앞 턴의 Stop 이 늦게 도착할 수 있다. 세션이 다른 턴을 진행하는 동안 지시를 보내면 사용자의 입력과 섞인다.

## Decision

v2 계약에 `agent` command 를 둔다. provider 는 결과 수집을 구현한 `claude`·`codex` 만 받는다.

- 턴 경계는 provider 플러그인이 훅에서 보고한다(`agent.task_turn_report`. 다른 `agent.*` 메서드처럼 `agent` 권한을 요구하고, 호출 플러그인이 그 provider namespace 를 소유해야 한다). 시작은 프롬프트 제출, 끝은 턴을 끝내는 Stop(최종 답변 포함)이나 오류다. 호스트는 surface 에 묶인 회차가 있을 때만 적용한다.
- 턴 표는 surface 마다 회차 하나만 묶는다. 기존 세션은 idle 이고 묶을 수 있을 때만 지시를 보내며, 그 뒤 시작 보고를 받은 다음의 종료만 이 회차의 것으로 본다. 새 세션은 spawn 의 지시가 첫 턴이다.
- 기본 출력은 `string`(최종 답변)이다. 다른 출력 타입은 같은 회차의 명시 제출(`agent.task_submit_result`)이 필요하다. 제출은 도착할 때 출력 타입으로 검사하고 턴이 끝날 때 결과가 된다. 제출 성공은 task 성공이 아니다. 같은 회차의 다른 값은 거절한다.
- idle 만으로는 성공하지 않는다. 결과 없이 끝난 턴은 `result_missing`, 오류로 끝난 턴은 `agent_turn_error`, 세션 종료는 `agent_exited`, 기한 초과는 `timed_out`, provider 호출 불가와 재시작으로 잃은 귀속은 `agent_unavailable` 로 끝나고 `typed_result.error.code` 로 구별한다. 입력 대기는 실패가 아니라 Running 의 `awaiting_input` phase 다.
- 턴 표는 영속하지 않는다. 재시작 뒤에는 보고를 어느 회차에 귀속할지 알 수 없으므로 실행 중이던 agent task 는 `agent_unavailable` 로 끝난다.
- task 는 세션을 닫지 않는다. 취소·종결 때 턴 묶음만 푼다.

현재 동작은 [agent runner 가이드](../dev-guide/agent-runner.md)의 "agent task" 절에 있다.

## Consequences

- 에이전트의 답이나 구조화 판정을 binding·전이 조건으로 바로 쓸 수 있다.
- 사용자가 쓰는 세션을 task 에 넘겨도 사용자의 턴과 섞이지 않는다. 대신 사용자가 세션을 계속 쓰면 task 는 기다린다(`timeout_ms` 로 상한을 둔다).
- 턴 보고를 구현하지 않은 provider 는 agent task 로 쓸 수 없다. 이전처럼 `custom` 으로 부를 수 있다.
- 결과를 내지 않고 턴을 끝낸 에이전트는 다시 묻지 않고 실패한다. 같은 회차에서 판정을 바꿀 수도 없다.
- 재시작하면 실행 중이던 agent task 는 실패하고, 세션에 남은 답은 결과가 되지 않는다.

## Alternatives Considered

- **idle 전이를 턴 끝으로 본다.** 사용자의 턴과 늦게 온 앞 턴의 Stop 을 구별할 수 없다.
- **화면 출력에서 답을 읽는다.** 렌더링·줄바꿈에 따라 값이 바뀌고 구조화 값을 검증할 수 없다.
- **최종 답변에서 JSON 을 파싱해 구조화 출력으로 쓴다.** 답변 형식이 어긋나면 업무 판단 없이 실패하고, 어느 부분이 결과인지 정할 규칙이 생긴다. 명시 제출은 도착 즉시 검사해 에이전트가 같은 턴에서 고칠 수 있다.
- **턴 표를 저장소에 영속한다.** 재시작 사이에 온 보고와 provider 세션의 상태를 맞출 수단이 없어, 영속해도 귀속을 보장할 수 없다.

## Reconsideration Triggers

코드와 설정에서 확인:

- claude·codex 외의 provider 가 턴 경계를 보고하게 되면 provider 목록을 넓힌다.
- provider 가 턴 id 를 지시와 함께 돌려주게 되면 시작 보고 대신 턴 id 로 귀속한다.

실행 결과로 확인:

- `result_missing` 이 자주 나서 재지시가 필요하면 추가 턴 대기를 검토한다.

## References

- `crates/tasty-agent/src/task/agent.rs` — agent command 계약, 기본 출력, 제출 안내.
- `crates/tasty-task-runtime/src/agent_turns.rs` — 턴 표와 판정.
- `crates/tasty-task-runtime/src/runner_host/agent.rs` — 실행과 지시 전달.
