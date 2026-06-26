# 마우스 입력 계층 (Input Layer)

윈도우 내부에서 마우스 이벤트(클릭·이동·스크롤)와 커서 아이콘이 **렌더링 z-order 와 일치하는 입력 z-order** 를 따른다 — 화면에서 위에 그려진 요소가 입력을 먼저 받고, 가려진 요소는 받지 않는다.

## 기본 동작: 소비 (Consume)

z-order 최상위부터 hit-test 한다. 좌표가 어떤 레이어 영역 안이면 그 레이어가 이벤트를 **소비**하고, 하위로 전달하지 않는다. 투과(pass-through)는 기본이 아니다.

## 입력 계층 순서

마우스 핸들러(`src/view/main/mouse.rs`)는 상위부터 차례로 가드한다:

| 순서 | 레이어 | 판정 | 동작 |
|------|--------|------|------|
| 1 | egui 위젯 (사이드바·탭바·오버레이) | `egui_consumed` | egui 가 소비하면 종료 |
| 2 | 모달/오버레이 | `overlay_open` | 열려 있으면 터미널 입력 차단 |
| 3 | Popup | `state.popup_hovered` | 팝업 위면 소비(터미널 무시) |
| 4 | Banner | `state.banner_hovered` | 배너 위면 소비(터미널 무시) |
| 5 | Divider | `find_*_divider_at`(threshold) | 분할 경계 안이면 소비(드래그 시작) |
| 6 | Terminal/Surface | — | 최하위, 콘텐츠가 처리 |

`handle_cursor_moved` / `handle_mouse_input` / `handle_mouse_wheel` 이 모두 같은 가드(`egui_consumed || overlay_open || state.popup_hovered || state.banner_hovered`)로 상위 레이어를 먼저 거른 뒤 divider → terminal 로 내려간다. 배너는 **자기 영역의 마우스를 소비**(뒤로 전파 X)하는 focus-less 오버레이라 popup 과 같은 precedence 에서 차단한다(Toast 는 입력을 통과시키므로 이 가드에 없다 — [banner 시스템](../design/systems/banner.md)).

### `popup_hovered` / `banner_hovered` 프레임 간 전달

`PopupManager::draw()`(`src/adapters/ui/popup/draw.rs`) / `BannerManager::draw()`(`src/adapters/ui/banner.rs`)는 렌더 시점에 호출되고 마우스 이벤트는 이벤트 시점에 처리돼 타이밍이 다르다. 그래서 draw 결과(`PopupDrawResult.hovered` / `BannerDrawResult.hovered`)를 `state.popup_hovered` / `state.banner_hovered`(`src/state.rs`)에 프레임 간 상태로 저장해 두고(배너는 `src/adapters/ui/notification.rs` 의 draw 패스에서 기록), 마우스 핸들러가 그 값을 참조한다. (상세 모델은 [popup 시스템](../design/systems/popup.md) · [banner 시스템](../design/systems/banner.md).)

## 커서 결정

이벤트를 소비한 레이어가 커서를 결정한다(`src/state/mouse.rs::winit_cursor_icon_at`).

| 레이어 | 커서 |
|--------|------|
| egui 위젯 | egui 결정(보통 Default/PointingHand) |
| Popup 타이틀바 | Grab(드래그 이동) |
| Popup 콘텐츠 | Default(또는 콘텐츠별) |
| Banner | Default(또는 내부 action 위젯별) |
| Divider (수직 분할) | ResizeHorizontal |
| Divider (수평 분할) | ResizeVertical |
| Terminal | Text |
| 그 외 surface | None(시스템 기본) |

## 관련

- [popup 시스템](../design/systems/popup.md) — 팝업 z-order·스코프·포커스
- [banner 시스템](../design/systems/banner.md) — 배너 마우스 소비·스코프·발화 정책
- [dev-guide/popup-implementation](../dev-guide/popup-implementation.md) — 팝업 추가 절차
- [multi-window](multi-window.md) — 윈도우 단위 구조
