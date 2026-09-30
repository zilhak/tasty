# ADR-0057: 변경 요청 재시도는 호출자별 명령 identity로 구분하고 기록 범위에서는 이벤트와 함께 확정한다

- **Status**: Accepted — 구현 상태: 단계적 이행 중. 현재는 모든 경로가 메모리 응답 저장소를 사용하며, 기록 범위의 영속 identity는 구조 저널을 활성화할 때 함께 적용한다
- **Date**: 2026-09-30
- **Tags**: ipc, idempotency, retry, event-sourcing
- **Group**: foundation

## Context

응답 유실 뒤 다시 보낸 요청과 사용자가 같은 작업을 새로 요청한 것은 params만으로 구분할 수 없다.
JSON-RPC ID는 응답 대응용이며 재연결을 넘는 중복 방지 키가 아니다.

[ADR-0005](0005-idempotent-mutation-retries.md)는 호출자별 `idempotency_key`와 메모리 응답 저장소를 선택했다.
저장소는 시간·항목 수·바이트로 제한되고, 퇴출되거나 재시작 뒤 사라진 키는 새 요청처럼 실행될 수 있었다.
당시에는 변경이 메모리 상태에만 반영되어 재시작 뒤 이전 변경 자체가 남지 않았으므로 이 한계가 허용됐다.

[ADR-0055](0055-structural-domain-event-sourcing.md)로 구조 변경이 영속 이벤트로 확정되면 이 전제가 깨진다.
이벤트가 commit된 뒤 응답 전에 종료되면 변경은 남고 키는 사라진다. 같은 키로 재시도하면 같은 변경을 두 번 실행한다.
또 닫기가 성공한 뒤의 재시도가 대상 존재 검사에서 not-found로 막히면 원래 결과에 도달하지 못한다.

## Decision

재시도 식별의 기본 계약은 유지한다.

- 메서드는 반복 전달 결과에 따라 Read·Idempotent·Mutate를 선언한다. 이름만으로 안전성을 추정하지 않는다.
- Mutate 요청의 재시도는 요청 메시지의 `idempotency_key`로 구분한다. 연결 대신 호출자 종류·ID와 키를 함께 사용한다.
- 진행 중인 같은 요청은 첫 실행에 합류하고, 완료된 요청은 저장된 결과를 돌려준다. 다른 요청에 키를 재사용하면 실행하지 않고 충돌로 답한다.
- 호스트가 아는 Mutate 이름은 engine·App·GUI debug·plugin forward 어느 경로에서도 같은 저장 계약을 거친다.
  plugin 고유 이름과 PTY raw 입력은 호스트가 의미를 모르므로 이 계약 밖에 둔다.
- 메서드별 KeyContract(Kept·Unneeded·Outside)와 capability 버전을 유지한다. client는 필요한 버전을 확인하기 전에 키가 필요한 변경 요청을 보내지 않는다.
  멱등 키 기능 버전 1은 engine, 버전 2는 App 경로, 버전 3은 GUI debug와 호스트 이름의 namespace forward를 포함한다. 구 서버는 모르는 키를 무시할 수 있다.
- 같은 키·같은 요청의 저장 응답에는 `idempotent_replay:true`를 붙인다. 이 표지는 답이 재생됐음을 뜻하며, 표지가 없다는 것만으로 계약 밖이라고 판단하지 않는다.
- 메모리 범위에서 항목이 퇴출되면 같은 키가 다시 실행될 수 있다. 다만 응답만 너무 커서 버렸을 때는 키를 남겨 `-32064`로 실행 완료·응답 없음 상태를 알린다.
  응답 없이 채널이 닫힌 항목은 저장소에서 제거한다.
- 권한·요청 수 상한·호출 빈도 제한에 따른 거절은 저장하지 않는다. 키 형식은 라우팅 전 공통 검사에서 확인한다.

구조 저널 범위에서는 키를 명령 identity로 영속한다.

- CommandExecutor는 현재 호출자의 권한을 먼저 확인한 뒤, 대상 존재 검사와 포커스 해소보다 앞서 caller scope·key·요청 digest로 기존 명령을 찾는다.
  기존 명령이 있으면 저장된 대상과 결과·진행 상태를 사용한다. 대상 생략 요청의 재시도에서 현재 포커스를 다시 읽지 않는다.
- 신규 명령만 대상을 해소하며, 해소한 대상과 입력을 명령 기록에 남긴다.
- 명령 identity·이벤트·effect 의무를 하나의 transaction으로 확정한다. 같은 키의 동시 실행은 저장소의 유일성 제약으로 합류한다.
- 응답 본문 cache와 명령 identity를 구분한다. 응답 본문을 퇴출해도 identity가 남아 있으면 새 요청으로 실행하지 않고 결과를 재구성하거나 실행 완료·응답 없음으로 답한다.
- 같은 저널을 이어서 쓰는 재시작에서는 중복 적용을 막는다. 보존 기간이 끝난 identity의 의미는 그 저널과 함께 끝나며 그 밖의 보장을 알리지 않는다.
  복원 자료를 새 저널로 가져오는 경우에는 key namespace를 새로 만들고 원래 명령을 새 자원에 재실행하지 않는다.
- 기록 범위 밖의 Mutate는 기존처럼 제한된 메모리 저장소를 쓴다.
- 보장 범위가 경로마다 다르므로 capability 문구를 실제 보장별로 새로 명시한다. 일부 범위의 영속 보장을 전역 보장으로 알리지 않는다.
  capability를 모르는 기존 client에 재시작을 넘는 보장을 주장하지 않는다.

## Consequences

기록 범위에서는 commit 뒤 crash나 응답 유실이 두 번째 효과로 이어지지 않는다. 이미 닫힌 대상의 닫기 재시도도 원래 결과를 받는다.
기존 client의 키 사용법과 응답 형식은 바뀌지 않는다.

같은 client가 보낸 요청이라도 대상 범위에 따라 보장이 다르다. 영속 범위와 메모리 범위를 capability와 가이드에 함께 적어야 한다.
명령 기록이 저널에 쌓이므로 보존 기간과 정리 정책이 필요하다. 이 결정은 외부 OS 작업의 정확히 한 번 실행을 보장하지 않는다.

지연 응답의 relay 비용과 relay 생성 실패 시 키 없이 실행되는 현재 예외는 메모리 범위에서 남는다. 실제 GUI 루프의 모든 우회 경로를 행동 테스트가 검증하는 상태도 아니다. 기록 범위로 옮겨진 경로에서는 이 예외를 허용하지 않는다.

## Alternatives Considered

- 메모리 저장소만 유지하는 안: 영속 이벤트와 휘발 키가 갈라져 crash 뒤 같은 변경을 다시 실행한다.
- params나 RPC ID로 중복을 판단하는 안: 정당한 반복 작업을 없애거나 재연결을 놓친다.
- 대상 존재 검사 뒤에 키를 조회하는 안: 닫기처럼 대상을 없애는 요청의 재시도가 원래 결과 대신 not-found를 받는다.
- 모든 Mutate를 한꺼번에 영속 identity로 옮기는 안: 기록 범위 밖 서비스는 이벤트와 같은 transaction이 없어 영속 키가 실제 변경과 원자적이지 않다.
- plugin 고유 이름까지 중복 제거하는 안: target plugin이 소유한 실행 계약을 바꾼다.
- 진행 중 재시도를 무조건 거절하는 안: 생성 ID처럼 다시 조회하기 어려운 결과를 전달하지 못한다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 기록 범위가 넓어지면 해당 메서드의 KeyContract와 capability 선언을 같은 변경에서 갱신한다.
- plugin 고유 메서드가 호스트에 의미를 선언하는 방식이 생기면 계약 밖 분류를 다시 본다.
- 여러 호스트가 같은 stream을 쓰게 되면 identity의 범위와 유일성 제약을 다시 정한다.

### 실행 결과로 확인

- 정상 재시도가 메모리 보존 범위를 자주 넘거나 relay가 누적되면 상한·실행 방식을 재검토한다.
- 라우터·forward·GUI 루프를 바꾸면 실제 실행 횟수와 재생 여부를 확인한다. plugin 고유 이름이 저장소에 들어가지 않는 대조와 완료 전 재시도의 합류도 확인한다.
- commit 직후 강제 종료 뒤 같은 키로 재시도해 변경이 한 번만 남는지, 닫기 재시도가 원래 결과를 받는지 확인한다.

## References

- 대체 대상: [ADR-0005](0005-idempotent-mutation-retries.md)
- [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0004](0004-ipc-discovery-and-errors.md) · [ADR-0012](0012-request-admission-and-isolation.md)
- [멱등 키 운영 계약](../dev-guide/api-conventions.md#변경-명령의-재시도는-키로-구별한다)
- [재시도 진단](../architecture/ipc-server.md#큐와-재시도-집계)
- 현재 구현: `src/adapters/ipc/handler/idempotency.rs`, `crates/tasty-ipc/src/method_meta.rs`.
