# ADR-0010: 저장소는 적용된 설정과 저장 실패를 구분해 알린다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: storage, sqlite, durability
- **Group**: foundation

## Context

SQLite가 설정 요청에 성공을 반환해도 실제 journal mode가 요청과 다를 수 있다.
DB를 열지 못해 in-memory로 대체한 상태와 정상 저장소도 성공 응답만 보면 구분되지 않는다.
설정 적용, 초기화 실패, 쓰기 실패와 재시작 후 보존 여부를 따로 알려야 한다.

## Decision

두 DB는 같은 pragma 적용 함수를 사용하고 네 설정의 요청값·실제값·오류를 보존한다.
파일 DB는 WAL, in-memory DB는 memory journal을 정상으로 판정한다.
설정이 적용되지 않은 열린 DB는 degraded로 표시하고 계속 사용한다.
`synchronous=NORMAL`의 내구성 수준은 이 진단 기능을 추가하면서 바꾸지 않는다.

state.db 초기화 실패는 사용자 안내 후 종료한다.
memory.db 초기화 실패는 기존대로 기본 설정의 in-memory SQLite로 대체해 계속 실행한다.
이 fallback은 init_failure와 degraded로 명시하고 부팅 로그에 재시작 후 소실을 알린다.
화면의 별도 fallback 안내는 현재 제공하지 않는다.

fallback에서 memory.db를 쓰는 IPC 성공 객체에는 durable:false를 추가한다.
대상은 memory 이름공간뿐 아니라 같은 DB를 쓰는 agent·approval·surface.meta·telemetry·session이다.
정상 응답 형식은 유지하며 저장하지 않는 메서드와 조회 부수 쓰기의 범위는 가이드에 명시한다.

초기화와 쓰기는 공용 StorageFailure 분류를 사용한다.
쓰기는 기존 오류 코드·문구를 유지하고 구조화된 원인을 추가한다.
commit 실패 시 quota와 변경 알림 버퍼는 갱신하지 않는다.
공유 memory mutex의 poison 보고 flag는 port가 소유하여 같은 잠금을 중복 보고하지 않는다.

## Consequences

에이전트는 세션 동안의 쓰기 성공과 재시작 후 보존을 구분할 수 있다.
운영자는 적용된 pragma와 fallback 원인을 Local 진단으로 확인할 수 있다.

정상 응답의 durable 필드 부재는 이 계약을 모르는 구 호스트와 구분되지 않는다.
NORMAL은 전원 장애 때 최신 commit을 보장하지 않는다.
READONLY 같은 오류의 기존 분류와 Read 메서드의 부수 쓰기에는 문서화된 한계가 남는다.
poison 복구 규칙은 memory store에 대한 것이며 프레임 stream에 일반화하지 않는다.

## Alternatives Considered

- pragma 반환값만 검사하면 성공으로 반환된 미적용 상태를 놓친다.
- 설정 차이를 모두 초기화 실패로 올리면 사용 가능했던 DB를 새로 거절한다.
- memory fallback을 없애거나 모든 쓰기를 거절하면 세션 동안 가능한 작업까지 중단된다.
- 오류마다 새 IPC 코드를 만들면 기존 코드 소비자의 호환이 깨진다.
- 정상 응답에도 durable:true를 일괄 추가하면 모든 성공 응답이 바뀐다.

## Reconsideration Triggers

실제 저장 정책이나 synchronous 수준을 바꿀 때 적용값·허용값·내구성 설명을 함께 갱신한다.
새 이름공간이 memory.db에 쓰면 durable 표시와 검증 대상에 추가한다.

사용자가 fallback을 모르고 작업을 잃는 사례가 생기면 화면 안내를 검토한다.
복수 memory store가 생기면 poison flag의 소유 범위도 다시 정한다.
잠금·디스크 상한 실패와 rollback을 실제 저장소로 확인하되 합성 오류 검증을 실제 I/O 장애 검증과 구분한다.

## References

- [설정 적용과 저장 실패](../design/systems/storage.md)
- [memory 가시성](../design/systems/memory.md)
- [오류와 poison 처리](../dev-guide/error-handling.md)
- 구현: `crates/tasty-memory/src/pragma.rs`, `crates/tasty-memory/src/failure.rs`, `src/adapters/ipc/handler/memory.rs`.
