# ADR-0012: 요청은 라우팅 전에 권한을 확인하고 사용자 입력과 분리한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: security, permissions, ipc
- **Group**: foundation

## Context

권한 표가 맞아도 전역 조회나 plugin 가로채기가 검사 전에 응답하면 권한을 우회한다.
반대로 진입점과 핸들러에서 같은 검사를 반복하면 호출 빈도 제한 토큰을 두 번 소비한다.
전용 API를 막아도 메모리 KV API로 호스트 기록을 읽거나 위조할 수 있으면 보호가 성립하지 않는다.

## Decision

모든 외부 IPC와 plugin host-call은 라우팅 전 공통 `check_request`에서
권한, telemetry cap, rate-limit을 순서대로 한 번 검사한다.
허용된 요청의 사용량 집계와 거절 audit도 요청 단위로 수행한다. Allow audit의 영속 여부는 저장 정책이 정한다.
비공개 타입 `CheckedRequest`로 검사 결과를 전달하고 외부에서 넘긴 검사 완료 플래그나 Local로 위장한 요청으로 대신하지 않는다.
새 host-call은 별도 요청이므로 새로 검사한다.

plugin 고유 namespace는 권한 검사를 받는 호출자에게 ipc.invoke 토큰을 요구한다.
소유 plugin의 자기 호출만 면제하고 같은 문자열의 agent ID에는 면제하지 않는다.
자식 권한은 부분집합으로 위임하되 소유 plugin은 자기 namespace 토큰을 나눠 줄 수 있다.
호스트 표에 정확한 이름이 있는 메서드는 그 표의 권한을 따른다.

권한 검사를 받는 호출자의 메모리 KV API에서 `tasty.` 접두사는 호스트 전용으로 예약한다.
키 지정은 거절하고 열거·개수에서는 제외한다. Local과 지원되는 전용 API는 기존 동작을 유지한다.

사용자 입력·포커스 재현은 debug 전용 모듈·표·라우터에 격리한다.
OS 전역 입력에는 debug에서도 명시적 실행 옵션을 요구한다.
대상 ID의 PTY에 데이터를 보내는 에이전트 작업과 OS 사용자 입력 주입을 구분한다.

로컬 telemetry 기록은 cap·이상 탐지에 필요하므로 전체 기록 해제 옵션을 두지 않는다.
권한 토큰은 호스트 IPC 제약이며 sandbox 없는 plugin의 직접 파일·네트워크 동작까지 막지는 않는다.

## Consequences

조기 응답과 일반 handler가 같은 권한·사용량 계약을 따른다.
다른 호출자의 호스트 기록 노출·위조와 release 입력 재현을 제한한다.

엔진 없는 GUI 구간에서는 Local 이외 호출자의 요청을 거절한다.
기존 저장소 오류의 fail-open 처리와 세션 발급 실패 뒤 무토큰 자식 실행 경로는 별도의 남은 한계다.
shared_buffer.create의 요구 토큰과 개수·총량 정책은 아직 정해지지 않았다.
기록을 멈추고 싶은 사용자를 위한 텔레메트리 전체 기록 해제 옵션은 제공하지 않는다.

## Alternatives Considered

- 일찍 반환하는 조회마다 검사를 추가하면 새 조회에서 검사를 빠뜨리기 쉽고 사용량 집계 위치도 달라진다.
- 진입점과 핸들러가 중복 검사하면 호출 빈도 제한 토큰을 실제로 두 번 소비한다.
- Global scope 전체를 plugin에서 막아도 다른 scope의 호스트 키 위조는 남고 정상 데이터 공유가 깨진다.
- runtime 옵션만 걸고 입력 재현 코드를 release에 남기면 빌드 격리 원칙을 지키지 못한다.
- telemetry를 무조건 끄면 그 기록을 사용하는 cap과 이상 탐지도 함께 무력화된다.

## Reconsideration Triggers

새 진입점·라우터·호출자·권한 위임 경로를 추가할 때 검사 횟수와 조기 응답을 함께 확인한다.
새 port 구현도 에이전트의 사용자 상태 접근 검사 대상에 포함한다.

OS sandbox나 telemetry 외부 전송이 생기면 권한과 기록 정책을 다시 정한다.
shared buffer 정책, 무토큰 fallback, engine 없는 agent 작업이 실제 요구가 되면 각각의 실행 계약을 해결한다.
이 문서의 미결 항목을 구현된 제한처럼 안내하지 않는다.

## References

- [플러그인 권한과 요청 검사](../dev-guide/plugin-permissions.md)
- [debug IPC 격리](../dev-guide/debug-ipc.md)
- [프로젝트 원칙](../identity.md)
- 구현: `src/adapters/ipc/handler/checked.rs`, `crates/tasty-ipc/src/caller.rs`, `src/adapters/ipc/handler/memory.rs`.
