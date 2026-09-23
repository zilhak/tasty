# ADR-0602: 도메인 작업과 자원 정리는 공용 실행 계층이 맡는다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: architecture, domain, ipc
- **Group**: foundation

## Context

IPC와 attach의 구조 변경이 서로 다른 함수를 사용하면 검증, 실패 문구와 자원 정리가 갈라진다.
도메인이 IPC 핸들러를 호출하거나 AppState 전체를 받으면 화면 없이 사용하는 코드도
상위 구현을 알아야 한다. 타입만 옮겨도 호출 경로가 그대로면 이 문제가 남는다.

## Decision

도메인은 `src/core`와 `src/ports`에 두고 상위 앱·adapter·GUI 모듈을 직접 참조하지 않는다.
지금은 별도 core 크레이트를 만들지 않고 모듈 경계와 검사로 의존 방향을 유지한다.

`core::structural_exec`가 구조 변경 검증·적용·cascade를 맡고 IPC와 forward가 함께 호출한다.
결과는 도메인 값으로 반환하며 wire 오류·응답 조립은 각 진입점에서 한다.
공용 정수·범위 검사는 `core::param_bag`에 두고 IPC 전용 nickname 해석까지 도메인에 넣지는 않는다.

닫힌 surface의 회수와 close 결과 변환은 `core::structural_cascade`의 공용 함수가 맡는다.
GUI에서만 소비하는 효과는 같은 본문 안에서 feature로 제한한다.
매니저가 필요한 mesh 정리는 실행 결과로 알리고 매니저 소유자가 수행한다.

도메인이 필요한 창 연산은 `CascadeWindow` 같은 trait으로 받는다.
IPC engine 핸들러는 `IpcWindow`와 요청별 `IntentOutbox`를 사용한다.
입장 검사가 만든 intent와 핸들러가 만든 intent의 순서를 유지하며 진입점이 창 큐로 옮긴다.
창 자체가 대상인 GUI·debug 핸들러는 별도 라우터에서 AppState를 받는다.
`EntryWindow`는 창 전체를 다시 꺼내는 접근자를 제공하지 않는다.

AppState를 둘째 상태 struct로 복제하지 않는다. GUI 전용 필드와 모듈을 컴파일에서 제외한다.
CoreState에도 사용자 포커스·선택·히스토리가 있으므로 포트 도입만으로 사용자 보호가 끝나지는 않는다.
요청 origin과 대상 선택 규칙을 함께 적용한다.

## Consequences

두 진입점에서 같은 검증과 정리를 수행하고 핸들러가 쓰는 창 연산을 타입으로 확인할 수 있다.
불필요한 AppState 인자가 없어 새 접근을 추가할 때 시그니처 변경이 드러난다.

같은 크레이트이므로 컴파일러만으로 상위 참조를 막지는 못한다.
경계 검사는 직접 경로와 GUI 의존을 확인하지만 형제 모듈을 통한 모든 전이 의존을 증명하지는 않는다.
상위 코드가 도메인 내부 필드를 직접 만지는 반대 방향도 이 검사의 대상이 아니다.

## Alternatives Considered

- core 크레이트 추출은 GUI 조건·형제 모듈·공개 API를 넓게 정리해야 하며 현재 비용에 비해 이득이 작다.
- 도메인에서 IPC 핸들러를 재사용하면 wire 왕복과 역의존이 남는다.
- 모든 창 상태를 노출하는 포트는 AppState 인자를 이름만 바꾼 것이어서 채택하지 않는다.
- intent를 반환값으로 흩어 보내면 호출자가 큐 적재를 빠뜨릴 수 있어 요청의 공용 출구를 둔다.

## Reconsideration Triggers

외부에서 도메인을 링크할 실제 소비자가 생기거나 편집 빌드 시간이 병목이 되면 크레이트 분리를 검토한다.
새 창 연산은 도메인 사실인지, 좁은 포트 연산인지, GUI 라우터의 일인지 먼저 정한다.

port 구현 파일을 추가하면 활성 상태 읽기 검사 대상에도 포함한다.
헤드리스 intent 적용이 AppState 없이 가능해지면 pump 인자를 다시 좁힌다.
IPC와 forward의 실패 문구 일치, 기존 문구 호환과 두 빌드의 cascade를 각각 검증한다.

## References

- [AppState 소유권과 port 사용](../dev-guide/app-state-ownership.md)
- [도메인 의존 경계](../architecture/index.md#도메인-경계--core--ports)
- [닫기 순서](../architecture/close-sequence.md)
- 구현: `src/core/structural_exec.rs`, `src/core/structural_cascade.rs`, `src/adapters/ipc/window_port.rs`.
