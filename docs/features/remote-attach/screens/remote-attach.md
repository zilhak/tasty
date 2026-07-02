# 원격 attach 화면 (GUI mirror)

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: 전용 화면 없음 — mirror 는 기존 워크스페이스 렌더러를 그대로 쓴다. 시각 요소는 사이드바/작업영역 문서로 위임(연결 개념).

원격 attach 는 대부분 headless/CLI 동작이고, GUI 로 보이는 것은 **mirror 워크스페이스 하나**와 **점유된 대상의 readonly 표시**뿐이다. 둘 다 별도 화면을 만들지 않고 기존 화면의 상태로 표현한다.

## 트리거

`tasty remote attach --into-gui …` 또는 자동 매핑된 워크스페이스 활성화 → 로컬 GUI 에 mirror 워크스페이스가 **추가**된다(포커스/active 강제 전환 없음 — 부모 기획).

## UI 요소 인벤토리

- **mirror 워크스페이스** — 사이드바에 일반 워크스페이스처럼 노출되되 **이름 앞 하늘색 `>_→` glyph**(collapsed 레일은 아바타 우하단 하늘색 corner chip)로 로컬과 구분한다(`Workspace.mirror`). status dot 은 실행상태 전용이라 mirror 색을 싣지 않는다. 사이드바 표현 → [`features/sidebar/`](../../sidebar/index.md).
- **mirror 콘텐츠** — 원격 트리(Pane/Tab/Surface)를 그대로 재구성해 [작업 영역](../../work-area/screens/work-area.md) 렌더러가 그린다. 별도 위젯 없음.
- **점유된 대상의 readonly 표시** — 서버측에서 다른 client 가 점유한 surface 는 내용은 보이되 조작이 차단된 readonly 로 렌더(작업영역의 deferred/readonly 상태와 동일 경로).

## 상태별 시각

- **mirror 활성** — 이름 앞 하늘색 mirror glyph(레일=우하단 chip). 원격 출력이 오면 즉시 갱신(3초 tick 은 backstop). status dot 은 별개로 실행상태(running/idle)만 표시.
- **점유 중(서버측)** — 그 surface 는 readonly. force-detach 되면 일반 surface 로 복귀.
- **세션 끊김** — 자동 재연결(지수 백오프) 동안의 표시도 mirror 워크스페이스 상태로 흡수.

## 시각 소스

전용 design-system 화면 없음. dot/워크스페이스 항목은 사이드바 시각 소스, mirror 타일은 작업영역 시각 소스를 따른다. 점유/mirror 는 *상태*일 뿐 새 레이아웃이 아니다.
</content>
