# 전체화면 무대 (Fullscreen stage)

- **Status**: Partial — 무대 코어(상태 · 정의 테이블 · 렌더 파이프라인 게이트) + OS 창 전체화면 전환 + 첫 콘텐츠(알림 무대) + 사용자 진입 경로(popup 타이틀바 전체화면 버튼) + 셸 종료 버튼까지 있다. 키보드 탈출 단축키 · 에이전트(debug IPC) 진입 · 입력 게이트는 아직 없다.
- **주체**: 로컬 사용자
- **ADR**: [ADR-0082](../../adr/0082-fullscreen-independent-stage.md)
- **코드**: `src/adapters/ui/fullscreen.rs` · `src/adapters/ui/fullscreen/defs.rs` · `src/adapters/ui/fullscreen/notifications.rs` · `src/adapters/ui/popup/draw.rs`(진입 버튼) · `src/state.rs` · `src/gfx/gpu.rs` · `src/view/main/redraw.rs`
- **화면**: 없음 — 무대 자체가 화면이다. 동작 모델은 [`design/systems/fullscreen-stage.md`](../../design/systems/fullscreen-stage.md)

## 목적

무언가를 창 전체로 크게 보여주기 위한 기반. tasty 에는 이 개념이 아예 없었다(winit
`set_fullscreen` 호출 0 건, View 안에서 요소가 작업영역을 독점하는 상태도 없음). 무대는 기존
레이아웃을 확대하는 대신 **창 전체를 쓰는 독립 표면**을 띄우고 뒤는 손대지 않는다 — 그래서
화면 rect 를 계산하는 기존 경로를 하나도 고치지 않는다(근거는 ADR-0082).

## 내부 동작

- **상태**: `AppState.fullscreen_stage: Option<StageState>` — 창(= `MainView`)당 최대 하나.
  창이 여럿이면 창마다 독립적으로 가질 수 있다. 영속화하지 않는다.
- **등록**: 무대에 올릴 수 있는 것은 `fullscreen::defs::all_defs()` 의 `StageDef` 뿐이다
  (`id` · `title_key` · `draw_fn` · `on_close`). 선언되지 않은 id 로 여는 시도는 거부된다.
  현재 등록된 무대: `blank`(셸 확인용 기준 무대) · `notifications`(알림 무대).
- **셸 chrome**: scrim + 상단 제목 + **종료 버튼**. 종료 버튼은 콘텐츠가 아니라 셸이 그린다 —
  무대 프레임에는 창 닫기 버튼(CSD 타이틀바)조차 없어, 콘텐츠 구현자가 빠뜨리면 창을 빠져나갈
  수 없는 상태가 되기 때문이다. 콘텐츠는 제목 띠 아래로 잘린 child `Ui` 만 받는다.
- **알림 무대**: popup 의 형상 함수(`notification::draw_notification_content_inner`)를 그대로
  호출한다 — popup 인스턴스가 아니라 같은 형상의 **별개 콘텐츠**이고, 목록 스크롤 위치는 무대
  자신의 것이라 popup 쪽과 섞이지 않는다(무대 종료 시 `on_close` 가 지운다).
- **진입/종료**: `AppState::open_fullscreen_stage(id)` / `close_fullscreen_stage()`. 다른 무대가
  올라와 있으면 그 무대를 닫고 교체하며, 같은 id 재진입은 no-op 이다. 종료는 **모든 닫힘
  경로가 지나는 유일한 지점**이고, 닫힌 id 가 훅 대기열을 거쳐 `on_close` 를 정확히 1 회
  발화시킨다.
- **렌더**: 무대가 켜져 있으면 프레임이 통째로 갈린다 — clear + 무대 콘텐츠만 그리고, 터미널
  글리프 · egui-mesh · attach mesh 합성 · host chrome(사이드바/탭바/상태바/popup/오버레이)은
  그리지 않는다.
- **무대 중에도 계속 도는 것**: PTY 출력 처리(스크롤백이 계속 쌓인다) · attach mesh relay ·
  offscreen surface 스크린샷 · window 스크린샷.
- **무대 중 동결되는 것**: 터미널 grid(cols/rows). 창 크기가 바뀌어도 진입 시점 값을 유지한다
  — 그래야 나올 때 원본이 리플로우되지 않는다. DPI 변경에 따른 신규 터미널 기본 grid 갱신은
  보류했다가 무대를 나온 첫 프레임에 1 회 적용한다.
- **WebView**: 네이티브 자식 뷰라 "안 그린다" 로는 사라지지 않는다. 무대는
  `has_egui_overlay_open()` 에 참여해 popup 과 같은 게이트로 `set_visible(false)` 를 받는다.

## 인터페이스

- **AI Agent (IPC/CLI)**: 없다. 무대는 화면 투영이고 진입은 사용자 조작이라, 에이전트가 여는
  release 표면을 두지 않는다(불가침 원칙 1). debug 격리 표면은 후속 작업에서 붙는다.
- **사용자 트리거**: 무대를 선언한 popup(`PopupDef.fullscreen_stage`)의 타이틀바 전체화면
  버튼 — 현재 알림 popup. 누르면 무대가 뜨고 **원본 popup 은 열린 채 남는다**. 종료는 무대
  우측 상단의 종료 버튼. 키보드 단축키는 아직 없다.

## 아직 없는 것

- **입력 게이트** — 무대는 화면만 갈아끼운다. 키보드/마우스 경로는 무대를 모르므로 무대 중에도
  키는 터미널로 가고 클릭은 뒤의 위젯 좌표로 판정된다.
- **키보드 탈출 수단** — 마우스 탈출은 셸이 공통 제공하는 종료 버튼으로 되지만, 무대 프레임은
  CSD 타이틀바까지 지우므로 그 버튼이 유일한 탈출 수단이다. 단축키는 아직 없다.

경계 상세는 [`design/systems/fullscreen-stage.md`](../../design/systems/fullscreen-stage.md) 의
"아직 없는 것" 절.

## OS 창 전환

무대가 서면 **창 자체가 모니터를 덮는다** — 무대의 경계는 작업영역이 아니라 OS 창까지다
(브라우저 Fullscreen API 와 같은 모델: 새 창을 만들지 않고 같은 창을 전환한다). 창이 있는
그 모니터를 덮으며 primary 로 튀지 않는다. 종료하면 진입 직전 창 상태로 되돌아간다 —
maximize 였으면 maximize 로, **사용자가 직접 만든 전체화면이었으면 그대로 유지한다**(무대는
자기가 만든 전환만 되돌린다). 뒤 터미널의 grid 는 두 전환 모두에서 불변이다.

동작 모델·플랫폼별 확인 결과(Wayland/Windows/macOS/멀티 모니터는 **미확인**)는
[`design/systems/fullscreen-stage.md` §OS 창 전환](../../design/systems/fullscreen-stage.md#os-창-전환).

## 비-목표 (Out of scope)

- 기존 요소(pane/tab/surface)를 뷰포트 크기로 리레이아웃하는 것 — 무대는 확대가 아니다.
- 무대 콘텐츠와 원본의 자동 동기화 — 무대에는 별개 데이터가 들어간다.
- 영속화 — 재시작이 전체화면 상태로 부팅되지 않는다.
- headless 대응 — 화면 투영이라 대응 도메인이 없다.

## Acceptance Criteria

단위 테스트로 상시 검증되는 것:

- [x] Given 정의 테이블에 없는 id When 무대 진입 Then 거부되고 무대가 서지 않는다
- [x] Given 무대 활성 When `has_egui_overlay_open()` 조회 Then true (WebView 숨김 게이트)
- [x] Given 무대 종료 When 다음 프레임 Then 닫힘 훅이 정확히 1 회 발화한다(무대/일반 프레임 양쪽)
- [x] Given 무대 A 활성 When 무대 B 진입 Then A 의 닫힘 훅이 1 회 발화하고 B 만 남는다
- [x] Given 진입 시점 창 상태 When 종료 시 복원 동작 결정 Then 일반/maximize 는 되돌리고 사용자 fullscreen 은 유지한다
- [x] Given fullscreen 단독 When 리사이즈 엣지 판정 Then 잠긴 것으로 본다(maximize 만 보지 않는다)
- [x] Given 무대를 선언한 popup When 타이틀바 전체화면 버튼 클릭 Then 그 무대가 뜨고 popup 은
      열린 채 남는다
- [x] Given 무대를 선언하지 않은 popup When 같은 좌표 클릭 Then 아무 무대도 뜨지 않는다
- [x] Given 알림 무대 활성 When 무대 종료 Then 무대 자체 상태(목록 스크롤)가 지워진다
- [x] Given popup 이 선언한 무대 id When 무대 테이블 조회 Then 반드시 존재하고, 그 popup 은
      headless 가 아니다

**미검증 — 살아 있는 GUI 가 필요해 자동 회귀가 없다.** 아래는 실제 창·PTY·GPU 가 있어야
재현되므로 headless 테스트로 고정할 수 없다. 진입 경로가 생겼으므로 실측 자체는 가능하며,
표시된 항목은 이번 라운드에 실측으로 1 회 확인했다(회귀를 잡지는 못한다).

- [ ] Given 무대 활성 When PTY 로 출력 발생 Then 무대를 나온 뒤 그 출력이 스크롤백에 있다
- [ ] Given 무대 진입 전후 When 터미널 grid 조회 Then cols/rows 가 동일하다(무대 중 창
      크기가 바뀌어도)
- [ ] Given 무대 활성 When `ui.screenshot`(window) Then 응답하고 결과에 무대만 찍힌다
      <!-- 이번 라운드 실측 확인: 무대 진입 후 window 캡처가 정상 응답 -->
- [ ] Given 무대 활성 When `ui.screenshot --surface <id>` Then 응답하고 그 surface 의 터미널
      내용이 찍힌다
- [ ] Given 일반 창에서 무대 진입 When 무대 종료 Then 진입 전 크기·위치의 일반 창으로 복귀
- [ ] Given maximize 창에서 무대 진입 When 무대 종료 Then maximize 로 복귀
- [ ] Given 이미 OS fullscreen 인 창에서 무대 진입 When 무대 종료 Then fullscreen 유지
- [ ] Given 창 2 개가 각각 무대 활성 When 한쪽만 종료 Then 다른 쪽 전체화면 유지
