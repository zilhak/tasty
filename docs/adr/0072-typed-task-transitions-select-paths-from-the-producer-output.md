# ADR-0072: v2 task 의 후속 경로는 생산자의 확정된 출력으로 고르고 고르지 않은 경로는 실패로 보지 않는다

- **Status**: Accepted
- **Date**: 2026-10-07
- **Tags**: agents, tasks, types, dag, routing
- **Group**: agents

## Context

v2 그래프는 depends_on·binding 으로 순서와 값을 잇지만, 결과에 따라 어느 후속 task 를 실행할지는 정할 수 없었다. 리뷰 결과가 pass 면 배포하고 revise 면 수정 작업을 돌리는 흐름을 만들려면 호출자가 결과를 읽고 다음 그래프를 다시 보내야 했다. 갈래가 나뉜 뒤 다시 합치는 흐름에서는, 기존 readiness 가 선행 하나라도 Skipped 면 하류를 건너뛰므로 실행되지 않은 갈래가 합류 task 를 막는다.

갈래를 고르는 판단이 실행 실패 처리와 섞이면 안 된다. 고르지 않은 갈래를 실패로 기록하면 fallback·continue_downstream 이 그 갈래를 되살리고, DAG 집계는 정상 종료한 그래프를 실패로 보인다.

## Decision

생산자 task 의 계약에 `transitions` 를 둔다. 조건은 그 task 의 확정된 출력(출력 검증을 통과한 값)만 읽는 제한된 순수 식이다.

- 조건은 비교·값 목록·논리 조합으로 된 제한된 형식이다. 셸·네트워크·시각·다른 task 의 상태를 읽지 않는다. 단건 생성은 전이를 받지 않는다(대상이 함께 제출돼야 한다).
- 그래프 제출 때 조건이 읽는 위치의 타입, 상수, case 겹침을 검사한다. 기본은 참인 case 가 정확히 하나여야 하는 배타 모드이고, 맞는 case 가 없을 때의 처리를 반드시 적는다. 제출 때 가릴 수 없는 다중 참은 실행 때 경로 오류로 처리한다.
- 고른 경로(`route`: 회차·참인 case·`otherwise` 여부·고른 대상)는 결과·종결과 같은 레코드 쓰기로 저장한다([ADR-0069](0069-typed-task-completion-is-one-write-per-attempt.md)). 경로를 고르지 못하면(값 없음·null·exclusive 다중 참) 그 task 는 실패 단계 `route` 로 끝나고 출력은 진단용으로 남는다.
- 고르지 않은 대상과, 들어오는 경로가 모두 선택되지 않은 task 는 실행 없이 Skipped 가 되고 `skip.reason: branch_not_selected` 를 남긴다. 실패 정책을 적용하지 않는다. 선행이 성공 결과를 내지 못해 건너뛴 v2 task 는 `upstream_unavailable` 과 그 선행·상태를 남긴다.
- 전이 대상은 제어 엣지로 들어온다. 그 엣지가 하나라도 고르기 전에는 실행하지 않는다. depends_on·binding·reduce 입력은 선택되지 않은 선행을 기다리지 않는다(합류). 선택된 선행이 실패하면 합류 task 는 실패 전파를 받는다.
- 선택되지 않을 수 있는 task 의 출력을 필수 입력으로 읽으면서 그 task 가 아닌 경로로도 실행될 수 있는 task 는 제출 때 거절한다. 대안 경로의 값은 `one_of`, 없어도 되는 값은 optional·default 로 적는다. 전이 대상은 continue_downstream 을 쓸 수 없고 fallback 대상일 수 없다.
- v2 task 의 `retry` 는 `reset_downstream` 을 거절한다. 하류는 이미 이전 회차의 실패나 경로로 판정됐고, 되감으면 같은 그래프에서 두 회차의 판단이 섞인다. 새 회차를 열면 저장된 경로와 skip 이유를 지운다.
- DAG 집계는 선택되지 않은 task 와 fallback 이 대신 성공한 실패를 실패로 보지 않는다. fallback 은 실패를 처리한 정상 경로이므로, 성공·선택되지 않음·fallback 이 대신한 실패만 있으면 그래프를 성공으로 본다. 이 집계 규칙은 v1 fallback 에도 적용한다. fallback 이 끝내 성공하지 못하면 실패다.
- DAG 집계는 저절로 진행할 수 있는 task 가 남아 있는 동안 실패가 섞여 있어도 진행 상태로 보인다. 저절로는 실행되지 않는 대기(선행 결과를 쓸 수 없는데 대기로 남은 task 와 그것을 전이적으로 기다리는 task)와 사람이 retry·cancel 해야 하는 `unknown` 은 진행할 수 있는 것으로 보지 않는다. 더 진행할 수 없을 때 복구되지 않은 실패가 있으면, 끝까지 성공한 갈래가 있는 그래프는 `partially_failed`, 없는 그래프는 `failed` 다. 갈래의 성공은 끝 task 로 센다. 끝 task 는 그룹 안에 하류가 없는 task 이며, 고르지 않아 건너뛴 task 와 fallback 간선은 하류로 보지 않는다. fallback 이 대신 성공한 끝도 성공으로 센다(복구한 실패를 실패로 세지 않는 것과 같은 이유).

조건 형식·모드·집계 필드는 [agent runner 가이드](../dev-guide/agent-runner.md)의 "전이 조건과 경로 선택" 절에 있다.

## Consequences

- 결과에 따른 갈래와 합류를 그래프 하나로 보낼 수 있다. 고른 경로는 결과와 함께 저장돼 재시작 뒤에도 같은 갈래를 따른다.
- 경로는 출력에서 결정적으로 정해지므로 완료 지문은 바뀌지 않는다. 같은 보고를 다시 내면 같은 경로가 나온다.
- 기존 v2 그래프의 판정이 바뀐다. 갈래가 없더라도 Skipped 선행의 하류는 전과 같이 실패 전파를 받지만, 건너뛴 이유(`skip`)가 레코드에 남는다. v1 task 의 판정은 그대로다.
- v2 task 를 고치고 하류까지 다시 돌리려면 새 그래프를 보내야 한다.
- fallback 으로 복구한 그래프는 v1·v2 모두 DAG 목록에서 `failed` 가 아니라 `succeeded` 로 보인다. 실패한 main 은 `state_counts.failed`·`recovered` 와 task 상태로 그대로 확인한다. main 의 실패로 건너뛴 소비자가 있으면 rollup 은 `skipped` 다.
- 한 갈래가 실패해도 다른 갈래가 도는 동안 DAG 목록은 진행 상태를 보이고, 끝난 뒤 성공한 갈래가 있으면 부분 오류로 보인다. `failed` 는 그래프가 더 진행할 수 없을 때만 나오므로, 실패를 바로 알아야 하는 클라이언트는 `state_counts.failed`·`recovered` 를 본다. `rollup_state` 값을 모두 나열해 처리하던 클라이언트는 새 값 `partially_failed` 를 처리해야 한다.
- 실패 없이 막힌 대기나 `unknown` 만 남은 그래프는 `waiting` 으로 남는다. 끝난 것으로 보이면 사람이 개입해야 하는 task 가 가려지기 때문이다.
- DAG 화면과 DOT 출력은 전이 간선을 depends_on 과 다른 모양으로 그리고 선택 상태를 굵기와 불투명도로 구분한다. 표현은 디자인을 받아 정했고 현재 모양은 [작업 러너 §전이 조건과 경로 선택](../dev-guide/agent-runner.md#전이-조건과-경로-선택)에 있다.

## Alternatives Considered

- **조건을 소비자(대상 task)에 둔다.** 대상마다 생산자의 출력 구조를 다시 적어야 하고, 대상들의 조건이 서로 배타적인지 한곳에서 검사할 수 없다.
- **일반 표현식 언어(CEL 등)를 쓴다.** 타입 검사와 겹침 검사를 제출 때 하기 어렵고, 조건이 출력 밖의 값을 읽을 여지가 생긴다.
- **선택되지 않음을 새 TaskState 로 둔다.** 모든 상태 소비자(필터·UI·CLI·이벤트)를 바꿔야 한다. Skipped 와 이유 필드로 같은 구분을 하면서 기존 소비자를 유지했다.
- **fallback 이 대신한 실패도 rollup 을 failed 로 둔다.** 전이의 미선택처럼 정해진 처리 경로를 따라 끝난 그래프를 실패로 보여, 실제로 복구하지 못한 그래프와 목록에서 구별되지 않는다.
- **실패가 생기면 즉시 rollup 을 failed 로 둔다.** 다른 갈래가 아직 도는 그래프가 목록에서 끝난 실패처럼 보이고, 실패 필터로 고른 그래프가 이후 성공 갈래를 더 낸다. 실패 여부는 `state_counts.failed` 로 바로 볼 수 있으므로 대표 상태는 진행 여부를 먼저 보인다.
- **성공한 task 가 하나라도 있으면 partially_failed 로 둔다.** 앞단만 성공하고 마지막이 실패한 단일 사슬도 부분 오류가 되어, 실제로 결과를 하나도 내지 못한 그래프와 구별되지 않는다. 끝까지 성공한 갈래가 있는지를 기준으로 했다.
- **retry 의 reset_downstream 으로 하류를 되감는다.** 이미 실행된 갈래의 결과를 되돌릴 수 없고, 되감은 하류가 이전 회차의 실패 전파와 다른 경로로 실행된다.

## Reconsideration Triggers

코드와 설정에서 확인:

- 조건이 출력 밖의 값(입력 snapshot, 메타데이터)을 읽어야 하는 요구가 생기면 결정성과 지문 규칙을 다시 본다.
- 그래프에 순환(반복)을 허용하게 되면 선택 여부 전파와 retry 규칙을 다시 정한다.

실행 결과로 확인:

- exclusive 다중 참으로 인한 실행 시 경로 오류가 자주 나면 제출 때 겹침 검사를 넓힌다.
- 끝 task 기준의 부분 오류가 사용자의 기대와 어긋나는 사례(성공한 끝이 있지만 주된 결과를 내지 못한 그래프를 부분 오류로 보는 등)가 반복되면 갈래의 성공을 세는 기준을 다시 본다. `agent dag-list` 의 `rollup_state` 와 `state_counts.succeeded_ends`, 해당 그래프의 task 상태를 함께 보고 확인한다.

## References

- `crates/tasty-agent/src/task/route.rs` — 전이 형식, 제출 검사, 경로 선택.
- `crates/tasty-agent/src/task/graph.rs` — 제어 엣지와 합류 판정.
- `crates/tasty-agent/src/task/store.rs`·`store/transition.rs`·`store/retry.rs` — 경로 저장, skip 이유, retry 규칙.
- `crates/tasty-agent/src/task/dag.rs` — DAG 집계(`rollup_state`, 막힌 대기, 끝 task).
- `crates/tasty-task-runtime/src/graph_view.rs` — `transition` 간선.
- [ADR-0068](0068-typed-task-graphs-activate-through-a-graph-record.md) — 그래프 활성화.
- [ADR-0069](0069-typed-task-completion-is-one-write-per-attempt.md) — 결과와 종결의 한 번 쓰기.
