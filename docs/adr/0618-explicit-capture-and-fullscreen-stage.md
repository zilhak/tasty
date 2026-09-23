# ADR-0618: 화면 캡처와 전체화면은 대상을 명확히 구분한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: screenshot, fullscreen
- **Group**: terminal

## Context

에이전트의 화면 관찰은 사용자 포커스를 바꾸지 않아야 한다. 화면 전체를 차지하는 사용자 도구도 기존 workspace 구조와 수명을 혼동하지 않아야 한다.

## Decision

스크린샷은 release에서 제공하는 관찰 기능으로 둔다. `ui.screenshot`은 surface ID 또는 window ID로 대상을 찾으며 포커스된 창을 기본값으로 삼지 않는다. 대상 생략은 해당 요청의 후보 창이 하나일 때만 허용한다.

터미널 surface 캡처는 자체 grid 크기의 오프스크린 텍스처로 렌더한다. 배경 탭과 워크스페이스도 활성화하지 않고 캡처한다. window 캡처는 chrome을 포함한 프레임이다.

사용자용 전체화면 콘텐츠는 기존 workspace/pane/tab/surface와 별개인 stage로 만든다. 기존 popup을 확대하지 않고 같은 형상의 별도 인스턴스를 구성한다. 창마다 최대 하나를 허용하고 정적 StageDef 테이블에 등록된 콘텐츠만 열 수 있다. stage 상태는 저장하지 않는다.

명시한 window ID는 main뿐 아니라 modal·preset 창도 캡처할 수 있다. 자동 선택은 main 창이 정확히 하나일 때만 가능하다. main이 없으면 그 사실을 오류로 반환하고 modal로 대체하지 않는다. window.list와 window.close는 main 창만 다룬다. 캡처를 허용한 것이 modal 조작 권한까지 넓히지는 않는다.

## Consequences

터미널 surface 캡처는 unfocused 색을 쓰고 커서·선택·IME 오버레이를 포함하지 않는다. 공유 projection uniform을 임시 변경한 뒤 반드시 복원해야 한다. 동기 GPU readback은 자주 실행하는 경로로 설계하지 않는다. 임의 경로에 파일을 쓰므로 local_only로 두고 plugin에 노출하지 않는다.

가려진 콘텐츠는 다시 그리지 않지만 원격 attach mesh 전달과 스크린샷은 계속 처리한다. GPU 분기는 surface 오프스크린 캡처 뒤, 일반 레이아웃 렌더 앞에 두며 window capture와 present는 유지한다. native WebView는 별도로 숨긴다. PTY grid 변경은 stage를 닫은 첫 프레임까지 보류한다. 원본과 stage 데이터가 함께 바뀌어야 하면 콘텐츠가 그 공유를 명시적으로 구현해야 한다.

modal ID는 OS 창 목록에서 찾는다. X11은 OS ID와 winit ID가 대응하지만 다른 플랫폼에서는 발견이 어려울 수 있다. 새 창 종류는 present 전에 screenshot readback을 처리해야 요청이 끝난다.

## Alternatives Considered

debug 전용으로 유지하면 에이전트가 자신의 결과를 관찰하기 어렵다. surface별 별도 메서드를 만들기보다 같은 screenshot 메서드의 대상 옵션을 사용한다. 모든 surface를 같은 GPU 경로로 캡처할 수는 없다. 특히 native webview는 OS가 합성한다.

기존 요소 확대는 GPU·egui·WebView·PTY resize·입력 hit test·방향 이동 등 여러 레이아웃 소비자를 함께 수정해야 한다. 전역 stage 상태는 여러 모니터의 독립 창 사용을 막는다. 임의 draw 클로저는 검증 도구가 지정할 콘텐츠 ID가 없으며 선언 목록을 우회한다. dirty 처리를 통째로 생략하면 원격 mesh 화면까지 멈춘다.

modal을 window.list에 넣으면 목록을 받은 모든 조작 API가 다시 제외해야 하므로 사용자 창이 닫힐 위험이 있다. window_kind 인자는 플랫폼별 ID 발견을 해결할 수 있지만 현재 API 범위를 넓히므로 보류한다.

## Reconsideration Triggers

비터미널 오프스크린 캡처, 실제 focus 색과 오버레이 포함, 배치 창 캡처, plugin 권한이나 경로 제한이 필요해지면 캡처 범위와 권한을 다시 정한다.

원본과 stage의 실시간 동기화, 한 창의 여러 stage, 공통 레이아웃 계산 도입, stage 중 DPI·모니터 변경 문제가 생기면 독립 인스턴스와 grid 보류 정책을 검토한다.

plugin 캡처 허용 또는 modal 열거가 필요하면 호출자의 신뢰 수준과 행동 대상 목록을 다시 정의한다. 다른 플랫폼에서 modal 자동 검증이 필요하면 window_kind나 debug 열거를 비교한다.

## References

- [스크린샷 검증](../ai-verification/screenshot-methods.md)
- [포커스 정책](../design/policies/focus.md)

- [전체화면 시스템](../design/systems/fullscreen-stage.md)
