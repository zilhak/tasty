# 윈도우 크롬 (CSD 타이틀바)

- **Status**: Partial (macOS 검증 완료 · Windows/Linux 코드 작성, 실기 검증 대기)
- **Surface**: 사용자 (마우스 조작 전용 — IPC/CLI 비노출)
- **Related ADR**: [ADR-0003](../adr/0003-client-side-decorations.md)
- **Related design**: [theme.md — CSD 타이틀바 토큰](../design/systems/theme.md), [key-mapping.md](../design/policies/key-mapping.md)

## 목적

tasty 는 OS 네이티브 데코레이션 대신 **클라이언트 사이드 데코레이션(CSD)** 으로 윈도우
상단에 자체 타이틀바를 그린다. 타이틀바 영역을 tasty 가 소유해야 향후 탭/상태를 크롬에
통합해 세로 공간을 절약할 수 있고, 3 OS 에서 일관된 시각·테마를 적용할 수 있다. 본 문서는
**현재 구현 상태**를 기술한다.

## 구조

- 윈도우 생성 시 OS별 CSD 속성을 적용한다 — `src/platform/window_chrome.rs::apply_csd_attributes`
  (첫 윈도우 `event_handler.rs`, 추가 윈도우 `window_lifecycle.rs` 공통 진입).
- 공통 타이틀바 어댑터: `src/adapters/ui/titlebar/`.
  - `mod.rs` — props 추출 + action → winit window 조작 브리지 (wrapper).
  - `view.rs` — 순수 view (props→actions). egui `TopBottomPanel::top` 으로 full-width
    36px 바 + 배경/하단 1px 보더(active/inactive 디밍) + 드래그/더블클릭 + Linux DE 버튼.
  - `caption.rs` — Windows 캡션 버튼(`#![cfg(target_os = "windows")]`).
  - `resize.rs` — Windows 리사이즈 보더 오버레이(`#![cfg(target_os = "windows")]`).
- 타이틀바는 `top_inset` 으로 사이드바·터미널 영역을 36px 아래로 밀어낸다
  (`titlebar::top_inset` → `compute_terminal_rect` 의 `top_inset` 인자, 단일 진실원).
- 색·길이는 모두 테마 CSD 타이틀바 토큰에서만 가져온다(하드코딩 없음).
  버튼 라벨은 `t("titlebar.{minimize,maximize,close}")` i18n 키.

## 사용자 행동 (UX)

공통 (3 OS):

- **창 이동**: 타이틀바의 비인터랙티브(드래그) 영역을 드래그 → `window.drag_window()`.
- **최대화 토글**: 드래그 영역 더블클릭 → `set_maximized(!is_maximized())`.
- 윈도우 포커스 상실 시 타이틀바 배경/전경이 inactive 토큰으로 디밍된다.

### macOS

- 네이티브 신호등(close/minimize/zoom)을 **유지**한다. `with_titlebar_transparent` +
  `with_fullsize_content_view` + `with_title_hidden` 조합으로 콘텐츠를 y=0 까지 확장하되
  OS 신호등(standardWindowButton)은 좌상단에 그대로 둔다.
- 신호등의 클릭 동작·hover 글리프·풀스크린·접근성·다크모드 디밍은 **모두 OS 가 처리**한다.
- tasty 는 신호등을 그리지 않고, 신호등 클러스터 폭(`MACOS_TRAFFIC_LIGHT_INSET` = 78pt)
  만큼 드래그 영역을 좌측에서 비워(carve-out) 신호등 클릭이 드래그로 새지 않게 한다.

### Windows

- OS 캡션/보더 제거(`with_decorations(false)`) + 드롭 섀도 복원(`with_undecorated_shadow(true)`).
- tasty 가 우측에 캡션 버튼 클러스터(좌→우: minimize, maximize/restore, close)를 직접 그린다.
  각 버튼 `caption_width`(46px) × 타이틀바 full height.
  - hover: minimize/maximize 는 overlay-hover 배경 + text-primary 글리프.
  - **close hover 만 시스템 red**(`accent_window_close` = `#c42b1c`, 테마 불변 OS 리터럴)
    배경 + 흰 글리프(`text_on_window_close`).
  - maximize 상태면 글리프가 restore(겹친 두 사각형)로 토글된다(`maximized` prop).
- **리사이즈 보더**: 데코 off 로 OS 리사이즈 보더가 사라지므로 tasty 가 윈도우 둘레에 egui
  인터랙티브 스트립(8방향 에지+코너)을 최상위 레이어로 깔고 `drag_resize_window` 로 OS
  리사이즈 루프를 띄운다. 우상단 캡션 클러스터와 겹치는 리사이즈 zone 은 carve-out 한다.
  최대화 상태에서는 보더를 깔지 않는다.

### Linux

- 네이티브 데코 제거(`with_decorations(false)`) + DE 가변 버튼을 직접 그린다.
  버튼 집합·순서·측면이 데이터 드리븐(`TitlebarControls { buttons, side }`).
  - 현재는 단일 기본 프리셋(우측 min·max·close, KDE-Breeze 류). 버튼은 `window_button_size`
    (24px) 원형, close hover 만 시스템 red.
- **가장자리 리사이즈**: 데코 없는 창에서 가장자리(`RESIZE_EDGE_MARGIN` = 8px) press 를
  감지해 `drag_resize_window` 로 리사이즈 시작 (`event_handler::handle_csd_resize_edge`,
  cross-platform API 지만 Linux 에서만 호출).

## 에이전트 행동 (CLI / IPC)

**없음 — 의도적 비-목표.** 타이틀바 버튼·드래그·리사이즈는 모두 **사용자 입력(마우스)**
이다. 원칙 1(사용자/에이전트 분리)에 따라 drag/minimize/maximize/close 를 프로그래밍적으로
트리거하는 IPC/CLI 는 release 표면에 노출하지 않는다.

## 현재 한계 (미구현 / 검증 대기)

- **Windows Snap Layouts 미구현**: maximize 버튼 위 hover 시 뜨는 Win11 Snap 플라이아웃은
  앱이 `WM_NCHITTEST` 에 `HTMAXBUTTON` 을 보고해야 DWM 이 그린다. winit 0.30 의
  `with_msg_hook` 으로는 NCHITTEST LRESULT 를 바꿀 수 없어 raw HWND 서브클래싱이 필요 →
  현재 범위 밖. (ADR-0003 재검토 트리거)
- **Linux Wayland 프레이밍 후속**: 상단 둥근 모서리(8px) + soft shadow 프레이밍은 윈도우
  투명화 + GPU 컴포지팅이 필요해 별도 후속. 현재는 리사이즈 엣지/이동만 처리.
- **Linux DE 감지 미구현**: GNOME(close만/우측) 등 DE별 버튼 프리셋 자동 감지·설정 노출은
  후속. 현재 단일 프리셋 고정.
- **실기 검증**: 현 개발 OS = macOS 라 Windows/Linux 캡션·리사이즈·DE 버튼은 **코드 작성
  완료(macOS 빌드/clippy/테스트 통과)** 상태이며 해당 OS 실기 동작 검증 대기.
- **디자인 보강 대기**: 디자인 시스템에 `titlebar_windows.jsx` / `titlebar_linux.jsx` 정식
  파일이 없어, Windows 캡션·Linux DE 버튼 구현은 P1 토큰 + 구현 계획 + OS 관습(Win11/
  freedesktop) 기반이다. 디자인-소스 일치를 위한 사후 명세화 요청이 제출되어 있다
  (`.claude-workspace/design-request/titlebar-{windows,linux}-*.md`).

## 관련 문서

- [ADR-0003 — 네이티브 데코 → CSD 전환](../adr/0003-client-side-decorations.md)
- [theme.md — CSD 타이틀바 토큰](../design/systems/theme.md)
- [key-mapping.md](../design/policies/key-mapping.md), [focus.md](../design/policies/focus.md)
