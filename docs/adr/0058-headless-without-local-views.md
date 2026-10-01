# ADR-0058: 헤드리스는 로컬 View 없이 같은 명령 실행 경계로 작업을 완료한다

- **Status**: Accepted — headless는 별도 CommandContext와 EngineSession을 사용하며 로컬 View 상태를 만들지 않는다. 구조 명령은 journal commit·publication과 effect 완료 경계를 공유한다. 일반 plugin bus·attach client·로컬 View 자동 복원을 새로 지원하지 않는다. 실제 headless 실행 검증은 별도다.
- **Date**: 2026-09-30
- **Tags**: headless, lifecycle, features, architecture
- **Group**: foundation

## Context

헤드리스에서 성공 응답만 보내고 변경을 처리하지 않으면 실제 상태는 바뀌지 않고 큐만 자란다.
반대로 GUI 전체를 포함하면 화면 없는 실행 형태의 의미가 없어진다. 어떤 작업을 함께 하고 어떤 작업을 지원하지 않는지 구분해야 한다.

[ADR-0003](0003-headless-behavior.md)은 헤드리스가 GUI와 같은 창 상태 구조를 재사용한다는 전제에서,
응답 전에 intent와 host event 큐를 한 번에 최대 8라운드 처리하는 방식을 택했다. 도메인 작업이 창 상태의 큐를 거쳐야 끝난다는
구조가 전제였다.

[ADR-0054](0054-app-core-view-layers-and-state-ownership.md)와 [ADR-0055](0055-structural-domain-event-sourcing.md)는 도메인 작업을
로컬 View 없이 CommandExecutor에서 완료하도록 정했다. 헤드리스가 큐를 따로 비워야 하는 이유는 이 구조에서 사라진다.
다만 헤드리스가 지원하는 제품 범위는 구현 방식과 별개로 유지해야 한다.

## Decision

헤드리스에는 로컬 View와 `ViewState`가 없다. 도메인 명령은 GUI와 같은 CommandExecutor 경계를 거쳐 응답 전에 완료한다.
헤드리스만을 위한 별도 도메인 실행 경로를 두지 않는다. 원격 표시에 필요한 순수 타입·계산·전송 adapter는 존재할 수 있다.

이행이 끝나기 전까지는 현재 방식을 유지한다. IPC·plugin 응답 전과 메인 루프 대기 전에 intent와 host event 큐를 처리하고,
한 번의 처리는 최대 8라운드이며 남은 것은 다음 처리에 넘긴다. 명령이 CommandExecutor로 합류한 경로부터 이 큐 처리를 걷어낸다.
8라운드 제한에 닿으면 제한을 올리기 전에 이벤트가 반복해서 생기는 원인을 찾는다.

지원 범위는 다음과 같다.

- 도메인 적용과 엔진 상태 변경은 수행하고 GUI redraw·메뉴·알림음은 실행하지 않는다.
  OSC 7 cwd는 헤드리스에서도 탭 이름과 레이아웃 변경 상태를 갱신한다.
- `HookFired`는 agent task 대기를 완료한다. host event의 일반 plugin bus 전달은 지원하지 않는다.
- plugin 조회는 설치·권한 부여·실행을 하지 않는다. 메타데이터 조회와 실제 기동을 분리하고, enable·disable은 공용 핸들러로 지정한 plugin만 처리한다.
  enable·disable 외의 설치·삭제·권한 변경 기능까지 지원한다고 해석하지 않는다.
  명시한 호스트 대상 ID는 실행 전에 확인하며 plugin 자체 ID 공간은 호스트가 추정하지 않는다.
- 헤드리스는 로컬 View의 레이아웃을 저장·복원하지 않는다. workspace는 프로세스 수명 동안 유지된다.
  복원 설정이 켜져 있으면 부팅 경고를 남기고 `system.info.layout_slot`은 `null`이다.
  구조 저널을 외부 자원 없이 재생·검증하는 기능은 가능하지만, 그것이 로컬 View 자동 복원을 지원한다는 뜻은 아니다.
- attach 서버 기능은 제공하고 attach client 기능은 제공하지 않는다. mirror 구조 요청은 실행할 전송 경로가 없으므로 큐에 넣지 않고 거절한다.
- 사용하지 않는 GUI 정의는 feature에서 제외한다. 테스트가 실제로 쓰는 정의만 test 조건에 남기고,
  특정 빌드에서만 읽히는 값에는 항목별 이유를 적은 `expect`를 쓴다. 모듈 전체 dead-code 허용은 공용 테스트와 생성 코드의 특별한 계약에만 둔다.

## Consequences

성공 응답 뒤의 조회가 실제 상태와 일치하고, 헤드리스와 GUI가 같은 도메인 경로를 쓰므로 한쪽에만 처리가 빠지는 결함이 줄어든다.
지원하지 않는 동작은 오류나 부팅 안내로 구분된다.

GUI와 헤드리스의 효과는 완전히 같지 않다. host event 소비, plugin 토글 통지, 레이아웃 복원, mirror 전달의 차이는 가이드에서 확인해야 한다.
이행 중에는 큐 처리와 새 경계가 경로별로 섞여 있으므로 기능을 헤드리스에 추가할 때 어느 경로를 쓰는지 확인하고 응답 전에 처리가 끝나는지 검증한다.

## Alternatives Considered

- 헤드리스 전용 큐 처리를 계속 유지하는 안: GUI와 헤드리스의 순서·완료 의미가 계속 갈라지고 새 경계와 이중 경로가 된다.
- 핸들러마다 큐를 우회해 직접 적용하는 안: 새 핸들러가 다시 처리를 빠뜨릴 수 있다.
- 헤드리스에 로컬 View 자동 복원을 추가하는 안: 셸 재기동·ID 변경·슬롯 충돌·지연 plugin 복원을 새로 설계해야 하며 요구가 없다.
- 도달할 수 없는 mirror 요청에 성공을 답하는 안: 다른 모듈의 feature 변경이 바로 결함이 된다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- plugin 이벤트 구독, attach client, 재시작 후 workspace 복원이 헤드리스의 제품 요구가 되면 지원 범위를 다시 정한다.
- 새 non-Domain 생산자나 host event 소비자가 생기면 헤드리스 처리 결과를 확인한다.
- 모든 도메인 명령이 CommandExecutor로 합류하면 intent 큐 처리와 8라운드 제한을 제거한다.

### 실행 결과로 확인

- 컴파일 경계를 바꾸면 GUI·헤드리스, debug·release, lib·all-targets 조합을 확인한다.
- 8라운드 제한에 닿는 사례가 관측되면 반복 발생 원인을 먼저 조사한다.

## References

- 대체 대상: [ADR-0003](0003-headless-behavior.md)
- [ADR-0054](0054-app-core-view-layers-and-state-ownership.md) · [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0061](0061-external-remote-module-and-attach-sync.md)
- [헤드리스 컴파일 경계와 동작 차이](../dev-guide/headless-build-boundaries.md)
- [헤드리스 IPC 지원표](../dev-guide/headless-ipc-surface.md)
- [레이아웃 저장](../features/layout-persistence/index.md)
- 현재 구현: `src/intent/headless.rs`, `src/boot/headless_dispatch.rs`.
