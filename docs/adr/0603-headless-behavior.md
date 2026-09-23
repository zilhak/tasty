# ADR-0603: 헤드리스는 화면 없이 완료할 수 있는 작업을 직접 처리한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: headless, lifecycle, features
- **Group**: foundation

## Context

헤드리스에서 성공 응답만 보내고 intent를 처리하지 않으면 실제 상태는 바뀌지 않고 큐만 자란다.
반대로 GUI 전체를 포함하면 화면 없는 실행 형태의 의미가 없어진다.
같이 해야 하는 작업과 지원하지 않는 작업을 구체적으로 구분해야 한다.

## Decision

헤드리스는 IPC·plugin 응답 전과 메인 루프 대기 전에 intent와 host event 큐를 처리한다.
도메인 적용과 엔진 상태 변경은 수행하고 GUI redraw·메뉴·알림음은 실행하지 않는다.
intent 처리는 한 번에 최대 8라운드이며 남은 것은 다음 처리에 넘긴다.
`HookFired`는 agent task 대기를 완료하고, host event의 일반 plugin bus 전달은 지원하지 않는다.
OSC 7 cwd는 헤드리스에서도 탭 이름과 레이아웃 변경 상태를 갱신한다.

조회는 plugin을 설치하거나 권한을 주거나 실행하지 않는다.
메타데이터 조회와 실제 기동을 분리하며 enable·disable은 공용 핸들러로 지정한 plugin만 처리한다.
다른 설치·삭제·권한 변경 기능까지 지원한다고 해석하지 않는다.
명시한 호스트 대상 ID는 실행 전에 확인하고 plugin 자체 ID 공간까지 호스트가 추정하지 않는다.

헤드리스는 레이아웃을 저장·복원하지 않는다. workspace는 프로세스 수명 동안 유지된다.
복원 설정이 켜져 있으면 부팅 경고를 남기고 `system.info.layout_slot`은 `null`이다.
attach 서버 기능과 attach client 기능을 구분한다. 헤드리스는 mirror 구조 요청을
실행할 전송 경로 없이 큐에 넣지 않고 거절한다.

사용되지 않는 GUI 정의는 feature에서 제외한다. 테스트가 실제 사용하는 정의만 test 조건에 남긴다.
생산은 되지만 특정 빌드에서만 읽히는 값은 항목별 이유 있는 expect를 사용한다.
모듈 전체 dead-code 허용은 binary마다 일부를 쓰는 공용 테스트와 생성 코드의 특별한 계약에만 둔다.

## Consequences

성공 응답 뒤에 조회한 상태가 일치하고 사용하지 않는 큐가 계속 쌓이지 않는다.
지원하지 않는 동작은 오류나 부팅 안내로 구분할 수 있다.

GUI와 헤드리스의 효과는 완전히 같지 않다. host event 소비, plugin 토글 통지,
레이아웃 복원과 mirror 전달의 차이를 가이드에서 확인해야 한다.
기능을 헤드리스에 추가할 때 cfg만 풀지 말고 생산자·소비자와 응답 전 완료를 함께 연결해야 한다.

## Alternatives Considered

- 핸들러마다 큐를 우회해 직접 적용하면 새 핸들러가 다시 처리를 빠뜨릴 수 있다.
- 큐를 비우기만 하면 메모리 증상은 없어져도 성공 뒤 무동작이 남는다.
- 자동 레이아웃 복원은 셸 재기동·ID 변경·슬롯 충돌·lazy plugin 복원 문제를 새로 설계해야 한다.
- 도달 불가능하다는 이유로 실행할 수 없는 mirror 요청에 성공을 답하면 다른 모듈의 feature 변경이 바로 결함이 된다.

## Reconsideration Triggers

새 non-Domain 생산자나 host event 소비자가 생기면 헤드리스 처리 결과를 확인한다.
plugin 이벤트 구독, attach client, 재시작 후 workspace 복원이 제품 요구가 되면 지원 범위를 다시 정한다.

컴파일 경계 변경은 GUI·헤드리스, debug·release, lib·all-targets 조합을 확인한다.
8라운드 제한에 닿으면 제한을 올리기 전에 재발화 원인을 살핀다.

## References

- [헤드리스 컴파일 경계와 동작 차이](../dev-guide/headless-build-boundaries.md)
- [헤드리스 IPC 지원표](../dev-guide/headless-ipc-surface.md)
- [레이아웃 저장](../features/layout-persistence/index.md)
- 구현: `src/intent/headless.rs`, `src/boot/headless_dispatch.rs`.
