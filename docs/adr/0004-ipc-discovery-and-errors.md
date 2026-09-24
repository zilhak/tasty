# ADR-0004: IPC는 지원 조건과 실패 원인을 응답으로 구분한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: ipc, compatibility, capabilities
- **Group**: foundation

## Context

호출자가 이름을 틀린 경우와 플랫폼·빌드·caller 때문에 실행할 수 없는 경우는 해결 방법이 다르다.
모두 Method not found로 답하거나 plugin을 거치며 코드를 뭉개면 호출자가 잘못된 재시도를 하게 된다.
패키지 버전만으로는 같은 버전의 GUI·헤드리스 기능 차이를 알 수도 없다.

## Decision

미등록 이름, 플랫폼 미지원, plugin-only, 현재 빌드의 구현 부재를 서로 다른 오류로 알린다.
정확한 코드와 대응은 API 가이드의 표를 따른다.
메서드 등록은 정확한 이름으로 확인하며 plugin prefix 아래 모든 이름을 등록된 호스트 메서드로 보지 않는다.
플랫폼 전용 구현에는 반대 조건에서도 이유를 돌려주는 분기를 둔다.

호스트 오류는 plugin host-call과 post-hook을 거쳐도 원래 error_code를 유지한다.
오류 문구의 기존 wrapper는 보존하며 코드가 없는 구 응답과 plugin 내부 오류만 기본 server error로 처리한다.
코드 전달과 사용자 표시 문구를 별도 계약으로 다룬다.

서버는 `system.info.capabilities`에 구현된 기능의 이름과 version을 선언한다.
기존 응답에 추가하여 구 client가 새 handshake를 몰라 연결을 잃지 않게 한다.
가능하면 실제 프로토콜 상수에서 version을 파생하고 미구현 기능을 미리 선언하지 않는다.
메서드의 baseline 포함 여부는 동결 파일에서 파생하며 정확한 도입 버전을 추정해 만들지 않는다.

호환성을 깨는 일반 변경은 deprecation 절차를 따른다.
보안·심각한 버그·불가침 원칙 위반 수정은 해당 범위에 한해 유예를 생략할 수 있다.
그 경우에도 기존 호출자를 가장 적게 깨는 수정안을 고르고 CHANGELOG에 구체적 사유를 적는다.

## Consequences

호출자는 이름, 실행 환경, 권한과 요청 상태 중 무엇을 확인해야 하는지 응답으로 알 수 있다.
새 기능을 쓰기 전에 capability를 확인할 수 있다.

오류 코드를 제어 흐름에 쓰는 client와 엄격한 응답 파서는 호환 검토가 필요하다.
구 SDK가 코드 필드를 보내지 않으면 원래 원인을 완전히 복원할 수 없다.
원칙 위반의 즉시 수정도 외부 호출자에게는 경고 기간 없는 동작 변경이다.

## Alternatives Considered

- 오류 문구만 바꾸고 코드를 유지하면 코드로 분기하는 client가 계속 잘못 판단한다.
- plugin마다 호스트 오류를 다시 분류하면 같은 실패가 plugin마다 달라진다.
- RPC 전체의 단일 버전은 필요한 기능만 선택하기 어렵고 별도 handshake는 구 client 연결을 깬다.
- 원칙 위반 예외를 모든 변경에 적용하면 정상 호환 절차가 의미를 잃는다.

## Reconsideration Triggers

플랫폼 지원 범위나 namespace 해소가 달라지면 오류 코드의 조건을 다시 확인한다.
plugin 내부 오류 때문에 외부 사용자가 고칠 수 없는 인자 오류를 받는 사례가 늘면 책임 구분을 검토한다.

동결 baseline·capability 의미·불가침 원칙이 바뀌거나 안정 1.x로 전환하면 관련 계약을 재검토한다.
사용자가 특정 break의 유예를 요구하거나 외부 client 피해가 확인되면 예외 정책도 다시 논의한다.

## References

- [API 규약](../dev-guide/api-conventions.md)
- [프로젝트 원칙](../identity.md)
- 구현: `crates/tasty-ipc/src/method_meta.rs`, `crates/tasty-ipc/src/capability.rs`, `crates/tasty-plugin-protocol/`.
