# 튜토리얼 (마커 오버레이 인앱 투어)

- **Status**: Partial (첫 주제 1개 · 4 step)
- **주체**: 로컬 사용자 (GUI 전용 — [주체](../../concepts/actors.md))
- **코드**: `src/adapters/ui/tutorial/`
- **화면**: 마커 오버레이 + 안내 말풍선 + 주제 목록 팝업 (갤러리 specimen: Overlays › Tutorial)

## 목적

처음 쓰는 사용자에게 tasty 의 화면 구조 개념을 UI 위에서 직접 가리켜 안내한다. "도구"
메뉴에서 열며, 각 step 이 화면의 특정 영역(워크스페이스/탭헤더/페인/서피스)에 **사각테두리
마커**를 얹고 그 옆 **말풍선**으로 개념을 설명한다. 마커는 위젯 자체를 건드리지 않고 좌표
위에 별도 도형을 최상위 z 로 그리는 방식이다.

## 내부 동작 (headless-valid)

- **구성 3요소**
  - **마커 오버레이** — 대상 rect 위에 그리는 정적 링(+정적 glow) + 스포트라이트 scrim(마커
    rect 만 밝게 남김). `Order::Tooltip` painter 로 최상위에 그리며 `pointer-events:none`
    (클릭은 하위로 통과). 메시지·심각도 없음(6번째 오버레이 개념 — [용어](../../concepts/ubiquitous-language.md)).
  - **안내 말풍선(callout)** — 244px 고정폭. `step/total` + dot rail, 제목, 본문,
    버튼 행(Skip · Back · Next). 마커를 가리키는 4방 tail. **edge-avoidance 배치**(선호순서
    below→above→right→left, 뷰포트 오버플로 시 flip, 8px 안전영역 clamp, clamp 후에도 tail 은
    마커 모서리를 계속 조준) — 순수 함수 `callout::place_callout`. 말풍선만 마우스를 소비.
  - **주제 목록 팝업** — `PopupDef`(CenteredFocused) 위 스크롤 리스트 + "진행" 버튼.
- **상태머신** — 목록팝업 --[진행]--> step0 --[Next]--> … --[Next on last]--> 목록 재open.
  Skip/Esc(any step) → 목록 재open(**완전 종료 아님**). Back → 이전 step(첫 step Back 숨김).
- **마커 좌표 해석** — step 의 `MarkerTarget`(ContentArea/TabHeader/Pane/Surface)을 매 프레임
  `LayoutContext`(pane/surface rect) · `terminal_rect`(콘텐츠 전체영역) · `tab_bar_height`
  로 재해석한다(정적 stale 없음). 첫 주제는 focused pane/surface 로 해석.
- **첫 주제** = "워크스페이스 · 페인 · 탭 · 서피스" 4 step: 워크스페이스(콘텐츠 전체영역) →
  탭 헤더(pane 상단 띠) → 페인(pane rect) → 서피스(surface rect). 마커가 점점 좁혀지며
  포함관계를 드러낸다.

## 인터페이스

- **AI Agent (IPC/CLI)**: **없음.** 튜토리얼은 사용자 조작 재현 계열(Toast/Banner/Modifier-hint
  와 동일) — 진입·진행·복귀를 IPC/CLI 로 발화하는 API 를 신설하지 않는다(불가침 원칙 1).
- **사용자 트리거**: 사이드바 "도구" 메뉴 → "튜토리얼" 클릭 → 주제 목록 팝업. 주제 선택 +
  "진행" → step 진행. Next/Back 이동, Skip/Esc 복귀. **최초 실행 자동 표시 없음.**

## 비-목표 (Out of scope)

- 리사이즈 중 마커 위치 실시간 추적은 매 프레임 재해석으로 자동 정합되지만, 스포트라이트 scrim
  OFF 토글(설정)은 아직 미구현(기본 ON) — 후속.
- 최초 실행 자동 표시 · 완료 상태 영속 · 첫 주제 외 추가 주제 · 개별 위젯 정밀 지시(egui
  `read_response` escape hatch)는 범위 밖(후속 주제에서).

## 구현

- `src/adapters/ui/tutorial/mod.rs` — `Topic`/`Step`/`MarkerTarget` 컴파일타임 정의,
  `TutorialRuntime`(AppState 필드), `resolve_marker_rect`(순수), `draw_tutorial_overlay`
  (오케스트레이션 — `src/adapters/ui/overlay.rs::draw_overlays` 말미 훅).
- `marker.rs` — `paint_marker` / `paint_spotlight_scrim`.
- `callout.rs` — `place_callout`(edge-avoidance 순수 함수 + 단위테스트) + `draw_callout`.
- `topic_popup.rs` — `draw_tutorial_topics_popup`(PopupDef draw_fn).
- 배선: `popup/defs.rs`(팝업 등록), `tools_menu.rs`(진입 항목), `state.rs`(런타임 필드),
  `lang/{en,ko,ja}.toml`(문자열). 시각 토큰은 design-system(Overlays › Tutorial specimen).

## Acceptance Criteria

- 도구 메뉴 "튜토리얼" → 주제 목록 팝업이 열린다.
- 주제 선택 + 진행 → 팝업 닫힘 → step0 마커+말풍선 진행.
- Next/Back 으로 step 이동, 각 step 마커 위치·말풍선 내용 갱신.
- Skip/Esc → 주제 목록 팝업 복귀, 마지막 step Next → 목록 재open.
- 마커/scrim 은 hit-transparent(클릭 통과), 말풍선만 마우스 소비.
