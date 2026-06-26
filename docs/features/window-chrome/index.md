# 윈도우 크롬 (Window chrome / CSD)

- **Status**: Implemented
- **주체**: 로컬 사용자 (GUI 전용 — 윈도우 조작)
- **ADR**: 없음 (CSD 데코 전략은 attach decision 과 무관, 원칙 4 크로스플랫폼)
- **코드**: `src/platform/window_chrome.rs` (CSD 속성·`resize_direction_at`), `src/adapters/ui/titlebar/` (`mod.rs`/`view.rs`/`caption.rs`), `src/view/main/mouse.rs` (통합 리사이즈 hit-test)
- **화면**: [screens/window-chrome.md](screens/window-chrome.md)

## 목적

[MainView](../main-view/index.md) 윈도우의 **CSD(Client-Side Decorations) 타이틀바**와 OS별 데코레이션 전략. winit `with_default_menu(false)` 로 tasty 가 창틀을 직접 소유하므로, 타이틀바·캡션 버튼·리사이즈 보더를 OS별로 직접 처리한다. 원칙 4(Windows/macOS/Linux 모두 1급)의 집행 지점.

## 내부 동작 (headless-valid)

### OS별 데코 전략 (`apply_csd_attributes`)

윈도우 생성 시 OS별로 다른 winit 속성을 적용한다:

- **macOS**: `titlebar_transparent` + `fullsize_content_view` + `title_hidden`. 타이틀바를 투명화하고 콘텐츠를 y=0 까지 확장하되 **OS 신호등(close/min/zoom)은 그대로 유지** — 클릭/hover/풀스크린/접근성/다크모드 디밍은 OS 가 처리. (`with_decorations(false)` 는 신호등까지 없애므로 안 씀.)
- **Linux**: `with_decorations(false)`. WM/컴포지터 데코를 끄고 tasty 가 DE 가변 캡션 버튼을 직접 그린다. 가장자리 리사이즈는 `drag_resize_window`, 이동은 `drag_window`(winit 0.30 표준).
- **Windows**: `with_decorations(false)` + `with_undecorated_shadow(true)`. OS 캡션/보더 제거 후 드롭 섀도는 복원, tasty 가 우측 캡션 버튼(min/max/restore/close)을 직접 그린다. 가장자리 리사이즈는 Linux 와 동일한 단일 MainView 경로로 처리(아래 "리사이즈 엣지").

### 타이틀바 (`titlebar/`)

순수 view(`draw_titlebar_view`)가 `TopBottomPanel::top` 으로 full-width 상단 바를 그리고, 입력을 `TitlebarAction` 으로 보고 → wrapper 가 winit 조작으로 브리지한다.

- **드래그 이동** (`StartDrag` → `drag_window`) — 빈 타이틀바 영역 드래그.
- **더블클릭 maximize** (`ToggleMaximize` → `set_maximized`).
- **캡션 버튼** (Linux/Windows, tasty 가 그림): `Minimize`/`ToggleMaximize`/`Close`. macOS 는 OS 신호등이라 tasty 가 안 그림(`controls=None`).
- **Close** 는 네이티브 `CloseRequested` 와 동일 라이프사이클(`AppEvent::CloseWindow`)로 라우팅 — 사용자 클릭이라 IPC 비노출(원칙 1).
- macOS 만 좌측 신호등 폭(`MACOS_TRAFFIC_LIGHT_INSET`)만큼 드래그 hit 영역을 carve-out.

### 상단 inset

타이틀바는 항상 그려지며 `top_inset`(= `titlebar_height` 토큰)을 반환한다 — `compute_terminal_rect` 가 작업 영역을 그만큼 아래에서 시작시킨다(상태바 `bottom_inset` 과 대칭).

### 리사이즈 엣지 (콘텐츠 우선 단일 모델)

데코 없는 Windows/Linux 창의 가장자리 리사이즈는 **단일 MainView 경로**로 통일된다(`src/view/main/mouse.rs`). 리사이즈는 **`egui_consumed == false`(콘텐츠가 포인터를 원하지 않음) AND 커서가 가장자리 margin 안 AND 비최대화 AND 오버레이/팝업/배너 없음**일 때만 발동한다 — 모든 egui 인터랙티브 콘텐츠(사이드바 버튼·캡션 버튼·상태바)가 자동으로 입력 우선권을 가지므로 가장자리 콘텐츠마다 carve-out 을 뚫을 필요가 없다(콘텐츠 우선 입력모델).

- `resize_direction_at` 는 좌표가 가장자리 margin(`RESIZE_EDGE_MARGIN`) 안이면 8방향 `ResizeDirection` 을 돌려주는 순수 함수(OS 무관 컴파일·테스트).
- `handle_mouse_input` 이 좌클릭 press 에서 hit-test → `Some(dir)` 이면 `drag_resize_window(dir)` 후 early-return.
- `handle_cursor_moved` 가 hover 방향을 `AppState.pending_resize_cursor` 에 저장하고, egui 프레임(`run_egui_frame`)이 `set_cursor_icon` 으로 ↔ 커서를 적용한다(egui 가 매 프레임 winit 커서를 덮으므로 프레임 내 적용 필수).
- **macOS 는 데코 있는 창**이라 OS 가 네이티브 보더에서 리사이즈를 처리한다 → 위 hit-test/커서 저장 블록은 `#[cfg(not(target_os = "macos"))]` 가드로 macOS 에서 컴파일·실행되지 않는다.

## 인터페이스

- **사용자 트리거**: 타이틀바 드래그(이동), 더블클릭(maximize), 캡션 버튼 클릭(min/max/close), 가장자리 드래그(리사이즈). macOS 신호등은 OS 처리.
- **AI Agent**: 없음 — 윈도우 데코/이동/리사이즈는 사용자 행동(원칙 1). 멀티 윈도우 *생성* 자체는 [main-view](../main-view/index.md).

## 비-목표 (Out of scope)

- **타이틀 텍스트 표시** — 현재 타이틀바는 드래그 영역 + OS별 컨트롤만(중앙 타이틀 없음).
- **윈도우 생성/생명주기** — [main-view](../main-view/index.md).
- **둥근 모서리/그림자 프레이밍**(Linux) — 윈도우 투명화 필요, 별도 후속.
- **단축키 정의** — `KeybindingSettings`.

## Acceptance Criteria

- [ ] Given 각 OS When 윈도우 생성 Then macOS=신호등 유지+콘텐츠 확장, Linux/Windows=OS 데코 제거 + tasty 캡션 버튼.
- [ ] Given 타이틀바 빈 영역 When 드래그 Then 윈도우가 이동한다.
- [ ] Given 타이틀바 When 더블클릭 Then maximize 토글된다.
- [ ] Given 캡션 close 버튼 When 클릭 Then 네이티브 닫기와 동일 경로로 윈도우가 닫힌다.
- [ ] Given 창 가장자리 When 드래그 Then 해당 방향으로 리사이즈된다.

> OS별 시각·동작이라 각 플랫폼 스크린샷 + 수동 윈도우 조작으로 검증. `resize_direction_at` 은 단위 테스트(OS 무관).

## 구현

- CSD 속성: `src/platform/window_chrome.rs` (`apply_csd_attributes`, `resize_direction_at`, `RESIZE_EDGE_MARGIN`).
- 타이틀바: `src/adapters/ui/titlebar/mod.rs`(wrapper `draw_titlebar`/`top_inset`/`os_controls`/`resize_cursor`), `view.rs`(순수 view + `TitlebarAction`/`WindowButton`/`ControlSide`), `caption.rs`(Windows 캡션).
- 가장자리 리사이즈: `src/view/main/mouse.rs`(`handle_mouse_input`/`handle_cursor_moved` hit-test), `AppState.pending_resize_cursor`(`src/state.rs`), 커서 적용 `src/gfx/gpu/egui_bridge.rs`.

## 화면

- [screens/window-chrome.md](screens/window-chrome.md) — OS별 타이틀바/캡션/신호등 배치.
</content>
