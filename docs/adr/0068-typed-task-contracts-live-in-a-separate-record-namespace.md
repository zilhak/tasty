# ADR-0068: 타입 계약을 가진 task는 별도 저장 namespace에 버전 envelope로 둔다

- **Status**: Accepted — 입력 바인딩, 상태 전이 세분화, IPC·CLI 생성은 아직 구현하지 않았다
- **Date**: 2026-10-06
- **Tags**: agents, tasks, types, compatibility, storage
- **Group**: agents

## Context

task 결과(`TaskResult.output`)는 임의 JSON이었다. 후속 작업이 결과를 값으로 쓰려면 출력의 모양을 선언하고 검사해야 하고,
"출력이 null로 확정됨"과 "출력이 아직 없음", 업무 값(`"revise"`, `false`)과 타입 오류를 구별해야 한다.
`run` 결과는 종료 코드와 stdout·stderr를 한 객체에 섞어 담았다.

이미 저장된 task와 이미 배포된 앱이 제약이다. 구버전 `TaskStore::list`는 `tasty.agent.task.` 아래 레코드 하나라도
해석하지 못하면 목록 전체를 실패시킨다. 해석에 성공하면 구버전 `Task`는 모르는 필드를 무시하고 그 task를 실행한다.
JSON 숫자는 소비자에 따라 f64로 읽혀 2^53을 넘는 정수가 바뀔 수 있다.

## Decision

task에 선택적 계약 `contract_version: 2`를 둔다. 계약이 없으면 v1이며 결과 형식·reducer·저장 형식을 소급해 바꾸지 않는다.

- v2 task는 `tasty.agent.typed_task.<id>` 키에 `{"record_format": "tasty.task/v2", "task": …}` envelope로 저장한다.
  v1 접두사와 겹치지 않아 구버전 목록 조회가 v2 레코드를 보지 않고, 구버전 모델은 envelope를 task로 읽지 못한다.
- 결과는 최종 출력(`has_output`, `output`)과 원시 응답·artifact·실패 단계·출처를 나눠 `typed_result`에 둔다.
  v1 필드 `result`에는 최종 출력을 투영한다.
- 결과 확정은 저장소의 `set_result`·`set_state`가 맡는다. 모든 완료 경로가 이 두 메서드를 지난다.
- 확정된 출력은 선언 타입을 아는 값(`TypedValue`)으로 든다. 메모리 안에서 int64는 i64이고, 이 타입의 `Serialize`가 항상 10진 문자열로 쓴다(proto3 JSON 관례). 직렬화 경로(저장, IPC, CLI, 결과만 내보내는 API)와 무관하게 같은 wire 형식이 나온다. 역직렬화는 스키마가 있어야 하며(`TypedValueSeed`), 문자열과 JSON 정수 토큰을 모두 받고 범위 초과와 소수는 거절한다. 스키마가 int64를 알려 주므로 string 타입과 혼동되지 않는다.
- reduce `all` 레코드의 `output`과 custom reducer stdin에는 각 입력의 선언 타입대로 직렬화한 값을 넣는다. `json` 타입 값은 무타입이라 안의 숫자를 바꾸지 않는다. 2^53을 넘는 정수의 정밀도가 필요하면 `int64`로 선언한다.
- v1이 v2 결과를 읽는 경로는 생성에서 거절한다. v1 출력 placeholder(`${task.<id>.output}`)가 v2 task를 가리키는 경우, v1 `Reduce`의 입력에 v2 task가 있는 경우, 단발 reduce에 v2 task를 준 경우다. v2 결과의 의미(계약 타입, run의 종료 코드)가 v1 경로로 조용히 바뀌어 넘어가지 않게 하기 위해서다. 허용 범위는 입력 binding이 정한다.
- 값 한도는 스키마 깊이 32, 값 깊이 64, 직렬화 크기 256KiB로 시작한다(`MAX_SCHEMA_DEPTH`·`MAX_VALUE_DEPTH`·`MAX_VALUE_BYTES`). memory 값 하나의 한도가 1MiB이고 v2 레코드에는 출력 외에 계약·raw 응답(run은 stdout·stderr 각 64KiB tail)·v1 투영이 함께 실리므로, 출력 하나를 그 4분의 1로 두었다. 깊이는 검증·직렬화가 재귀하는 깊이를 막는 값이며 사람이 쓰는 스키마·결과에 충분한 여유를 둔 초기값이다.
- 그 밖의 암묵 변환은 하지 않는다. 형식과 규칙은 [작업 러너 §v2 타입 계약](../dev-guide/agent-runner.md#v2-타입-계약-contract_version-2)에 있다.

## Consequences

구버전 앱이 v2 데이터를 실행하거나 v1 목록 조회에 실패하지 않는다. v1 레코드는 다시 저장해도 같은 JSON으로 남는다.

task 조회·삭제는 두 키를 확인해야 하고, 목록은 두 접두사를 읽는다. memory의 호스트 키 공간이 하나 늘었다.
int64 출력은 JavaScript를 지나도 정확하고, Rust 소비자(reducer, 조건 평가)는 정수를 그대로 받는다. 직렬화된 레코드·응답만 보면 같은 task라도 `raw.exit_code`는 숫자, `output`은 문자열이다. 역직렬화에는 스키마가 필요해 `TypedResult`는 단독으로 역직렬화하지 않고 `Task`(계약 포함)를 통해 읽는다. `json` 타입 값 안의 큰 정수는 JavaScript 도구에서 정밀도를 잃을 수 있다.
계약 형식을 바꿀 때는 `record_format`과 `contract_version`을 함께 올리고 새 namespace를 검토해야 한다.

## Alternatives Considered

- 같은 namespace에 버전 필드만 추가: 구버전이 계약을 무시하고 v2 task를 v1로 실행한다. 시험 `typed_tasks_live_outside_the_v1_namespace_and_old_readers_cannot_run_them`이 envelope 없는 레코드를 구버전 모델이 run으로 읽는 것을 보인다.
- 같은 namespace에 envelope: 구버전 목록 조회가 그 레코드를 해석하지 못해 그 workspace의 v1 task 목록 전체가 실패한다.
- 기존 v1 레코드를 v2로 이전: 저장된 DAG의 의미가 소급해 바뀌고, 이전 후에는 구버전으로 돌아갈 수 없다.
- int64를 JSON 정수 토큰으로 유지: Tasty의 Rust 경로는 정확하지만 JavaScript 도구를 지나면 2^53을 넘는 값이 바뀐다. 설계 요구(최소·최대·9007199254740993의 복원)를 어겨 채택하지 않았다.
- int64를 문자열로만 입력받기: 정수 토큰을 보내는 기존 도구가 모두 바뀌어야 한다. 읽을 때는 두 형식을 받고 쓸 때만 문자열로 정했다.

## Reconsideration Triggers

- JSON 숫자를 정확히 다루는 표준 수단(예: 대상 소비자 전부의 BigInt 대응)이 생기면 int64 문자열 표기를 다시 검토한다.
- 실제 결과가 256KiB 한도에 걸린다는 보고가 나오거나, raw를 별도 키로 옮겨 레코드 여유가 바뀌면 크기 한도를 다시 정한다. 깊이 한도는 정상 스키마가 걸릴 때 올린다.
- 입력 binding이 v1 placeholder·v1 reduce를 대신하면 v1 경로 거절 규칙을 binding 규칙으로 옮긴다.
- 구버전 앱과 같은 데이터 폴더를 공유할 필요가 사라지면 v1 레코드를 v2로 이전하고 namespace를 합칠지 검토한다.
- memory 값 크기 한도(현재 1MiB)나 계약 형식이 바뀌어 envelope 버전을 올릴 때.

## References

- [ADR-0042](0042-agent-coordination-and-task-views.md) — 작업 조율을 호스트가 맡고 저장 키 이름 규칙을 둔 결정
- [ADR-0062](0062-task-service-and-hook-runtime.md) — 작업 실행 소유
- [작업 러너](../dev-guide/agent-runner.md#v2-타입-계약-contract_version-2)
