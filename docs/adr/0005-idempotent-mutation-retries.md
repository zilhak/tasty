# ADR-0005: 변경 요청 재시도는 호출자별 멱등 키로 구분한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: ipc, idempotency, retry
- **Group**: foundation

## Context

응답 유실 뒤 다시 보낸 요청과 사용자가 같은 작업을 새로 요청한 것은 params만으로 구분할 수 없다.
JSON-RPC ID는 응답 대응용이며 재연결을 넘는 중복 방지 키가 아니다.
재시도 의도는 호출자가 명시하고 서버는 실제 적용 범위를 알려야 한다.

## Decision

메서드는 반복 전달 결과에 따라 Read·Idempotent·Mutate를 선언한다.
이름이 read나 set이라는 이유만으로 안전성을 추정하지 않는다.
Mutate 요청의 재시도는 봉투의 `idempotency_key`로 구분한다.
저장소는 연결 대신 caller 종류·ID와 키를 함께 사용한다.

호스트가 아는 Mutate 이름은 engine·App·GUI debug·plugin forward 어느 경로에서도 저장소를 거친다.
진행 중인 같은 요청은 첫 실행에 합류하고 완료된 요청은 저장된 답을 재생한다.
다른 요청에 키를 재사용하면 실행하지 않고 충돌로 답한다.
plugin 고유 이름은 호스트가 의미를 모르므로 이 계약 밖에 둔다.

메서드별 KeyContract는 Kept{since}·Unneeded·Outside다.
client는 요청 전 필요한 capability 버전을 확인한다.
멱등 키 기능 버전 1은 engine, 버전 2는 App, 버전 3은 GUI debug와 호스트 이름의 namespace forward를 포함한다.
구 서버가 모르는 키를 무시할 수 있으므로 확인 전에 변경 요청을 보내지 않는다.

저장소는 메모리에 두고 시간·항목 수·총 바이트·응답 크기를 제한한다.
퇴출되거나 재시작 뒤 사라진 키는 새 요청처럼 실행될 수 있다.
응답만 너무 커서 버렸을 때는 키를 남겨 실행 완료·응답 없음 상태로 답한다.
권한·cap·rate 거절은 저장하지 않는다. 키 형식은 라우팅 전 공통 검사에서 확인한다.

## Consequences

보존 범위 안에서 응답 유실이 두 번째 효과로 이어지는 것을 막는다.
`idempotent_replay`는 답이 재생됐음을 뜻하며 표지 부재만으로 계약 밖이라고 판단하지 않는다.

지연 응답은 relay 스레드와 요청 사본 비용을 쓴다.
현재 relay 생성 실패는 경고 뒤 키 없이 실행하는 예외이므로 그 경우 중복 방지를 보장하지 않는다.
응답 없이 채널이 닫힌 항목도 잊는다. 이 설계는 무제한·영속적인 exactly-once 약속이 아니다.
실제 GUI 루프의 모든 우회 경로를 행동 테스트가 검증하는 상태도 아니다.

## Alternatives Considered

- params나 RPC ID만으로 중복을 판단하면 정당한 반복 작업까지 없애거나 재연결을 놓친다.
- 모든 조회 응답을 저장하면 재조회가 오래된 상태를 돌려준다.
- 만료된 키를 영원히 기억하면 메모리 상한을 지킬 수 없다.
- plugin 고유 이름까지 임의로 중복 제거하면 target plugin이 소유한 실행 계약을 바꾼다.
- 진행 중 재시도를 무조건 거절하면 생성 ID처럼 다시 조회하기 어려운 결과를 전달하지 못한다.

## Reconsideration Triggers

정상 재시도가 보존 범위를 자주 넘거나 relay가 누적되면 상한·실행 방식을 재검토한다.
재시작을 넘는 보장이 필요하면 저장소와 복구 정책을 별도로 설계한다.

라우터·forward·GUI 루프 변경 시 실제 실행 횟수와 재생 여부를 확인한다.
특히 plugin 고유 이름이 저장소에 들어가지 않는 대조 사례와 완료 전 재시도의 합류를 확인한다.
새 경로의 선언 버전과 실제 저장 동작은 함께 갱신한다.

## References

- [멱등 키 운영 계약](../dev-guide/api-conventions.md#변경-명령의-재시도는-키로-구별한다)
- [재시도 진단](../architecture/ipc-server.md#큐와-재시도-집계)
- 구현: `src/adapters/ipc/handler/idempotency.rs`, `crates/tasty-ipc/src/method_meta.rs`.
