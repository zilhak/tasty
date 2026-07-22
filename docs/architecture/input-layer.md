# 마우스 입력 계층 (Input Layer)

윈도우 내부에서 마우스 이벤트(클릭·이동·스크롤)와 커서 아이콘이 **렌더링 z-order 와 일치하는 입력 z-order** 를 따른다 — 화면에서 위에 그려진 요소가 입력을 먼저 받고, 가려진 요소는 받지 않는다.

## 기본 동작: 소비 (Consume)

z-order 최상위부터 hit-test 한다. 좌표가 어떤 레이어 영역 안이면 그 레이어가 이벤트를 **소비**하고, 하위로 전달하지 않는다. 투과(pass-through)는 기본이 아니다.

## 입력 계층 순서

마우스 핸들러(`src/view/main/mouse.rs`)는 상위부터 차례로 가드한다:

| 순서 | 레이어 | 판정 | 동작 |
|------|--------|------|------|
| 1 | 모달/오버레이 | `overlay_open` | 열려 있으면 터미널 입력 차단 |
| 2 | Popup | `state.popup_hovered` | 팝업 위면 소비(터미널 무시) |
| 2b | Modifier-hint 오버레이 | `state.modifier_hint_hovered` | 오버레이 위면 소비 **+ 비활성 surface 전환도 차단**(popup 과 동급) — 드래그/리사이즈/X 클릭이 하위 surface 포커스로 안 샘 |
| 3 | **비활성 surface 전환** (좌클릭 press) | `surface_id_at_position != focused` | 첫 클릭은 surface 전환이 소비 — 그 위 배너/egui 위젯과 무관(아래 참고) |
| 4 | egui 위젯 (사이드바·탭바·오버레이) | `egui_consumed` | egui 가 소비하면 종료 |
| 5 | Banner | `state.banner_hovered` | 배너 위면 소비(터미널 무시) — **단 비활성 surface 전환에는 적용되지 않음** |
| 6 | Divider | `find_*_divider_at`(threshold) | 분할 경계 안이면 소비(드래그 시작) |
| 7 | Terminal/Surface | — | 최하위, 콘텐츠가 처리 |

`handle_cursor_moved` / `handle_mouse_input` / `handle_mouse_wheel` 이 모두 같은 가드(`egui_consumed || overlay_open || state.popup_hovered || state.banner_hovered || state.modifier_hint_hovered`)로 상위 레이어를 먼저 거른 뒤 divider → terminal 로 내려간다. 배너는 **자기 영역의 마우스를 소비**(뒤로 전파 X)하는 focus-less 오버레이라 popup 과 같은 precedence 에서 차단한다(Toast 는 입력을 통과시키므로 이 가드에 없다 — [banner 시스템](../design/systems/banner.md)).

**진행 중인 divider 드래그는 surface 콘텐츠보다 우선한다(순서 6 > 순서 7).** egui-mesh surface(마크다운·이미지)는 포인터 이동/버튼을 surface-local 로 forward 하고 소비하는 별도 경로(`egui_mesh_target_at`)를 갖는데, 이 forward 는 표의 순서 7(콘텐츠) 성격이므로 **`dragging_divider.is_some()` 일 때는 건너뛴다** — `handle_cursor_moved`(포인터 이동)와 `handle_mouse_input`(버튼)의 egui-mesh forward 가드에 `dragging_divider.is_none()` 조건을 둔다. 없으면 드래그 중 커서가 egui-mesh surface 영역으로 들어갈 때 forward 가 early-return 하여 divider 갱신이 멈추고(멈춤), release 도 forward 로 소비돼 드래그가 확정/해제되지 않는다(sticky). 터미널/explorer surface 는 egui-mesh 가 아니라 이 경로를 타지 않아 원래도 정상.

**Modifier-hint 오버레이**(modifier 홀드 시 뜨는 focus-less 패널)도 마우스를 소비하지만 배너보다 한 단계 강하다: 위 통합 가드(소비·휠·커서)에 더해 **click-to-activate 전환 가드**(`!popup_hovered && !modifier_hint_hovered`)에도 들어가, 오버레이 위 좌클릭이 하위 surface 로 **포커스를 옮기지 못하게** 막는다(popup 과 동급, banner 와 차이). 오버레이 드래그 이동·테두리/코너 리사이즈·X 클릭이 터미널 포커스·selection·마우스 리포트로 새지 않는다 — 키보드 포커스는 애초에 취득하지 않는다(원칙3, [banner 와 동일한 focus-less 성질]). 4지점 배선: `handle_mouse_input` 의 click-to-activate press 가드 + 통합 소비 가드, `handle_mouse_wheel`, `handle_cursor_moved`.

### 비활성 surface 클릭 = 전환 우선 (click-to-activate swallow)

**포커스되지 않은 surface 영역을 좌클릭하면, 그 위에 배너/egui 위젯이 있든 없든 그 첫 클릭은 "surface 전환" 이 통째로 소비한다** (macOS click-to-activate 모델). 이 단계는 통합 가드(`egui_consumed || … || banner_hovered`) **위**, modal/popup **아래**에 위치한다(`src/view/main/mouse.rs::handle_mouse_input`). 따라서:

- modal(`overlay_open`)·popup(`popup_hovered`)은 surface 비소속 독립 상위 레이어라 전환보다 **먼저** 배제된다 — 팝업/모달을 클릭해도 뒤 surface 로 포커스가 넘어가지 않는다.
- Banner 소비(순서 5)는 *동작*(action 위젯·마우스 리포트 차단)에는 적용되나 *비활성 surface 포커스 전환*에는 적용되지 않는다 — 배너 카드를 클릭해도 소속 surface 로 포커스가 간다. 이로써 마우스-캡쳐 배너 같은 persistent 배너가 떠 있어도 surface 전환이 막히지 않는다.
- **트레이드오프**: 비활성 surface 첫 클릭은 전환에만 쓰이고 그 자리 selection/cursor/마우스 리포트로 흐르지 않는다(한 번 더 클릭). 배경 캡쳐 TUI 로 마우스가 새는 것을 막는다.
- gating 은 `surface_id_at_position(x,y) != focused_surface_id` — 이미 활성인 surface 안 클릭은 전환 단계를 건너뛰어 정상(selection/리포트/divider)으로 흐른다. egui 크롬(사이드바·탭바)은 surface rect 밖이라 `surface_id_at_position == None` → 전환 대상 아님 → `egui_consumed` 로 소비.

### `popup_hovered` / `banner_hovered` / `modifier_hint_hovered` 프레임 간 전달

`PopupManager::draw()`(`src/adapters/ui/popup/draw.rs`) / `BannerManager::draw()`(`src/adapters/ui/banner.rs`) / `draw_modifier_hint()`(`src/adapters/ui/modifier_hint_overlay.rs`)는 렌더 시점에 호출되고 마우스 이벤트는 이벤트 시점에 처리돼 타이밍이 다르다. 그래서 draw 결과(`PopupDrawResult.hovered` / `BannerDrawResult.hovered` / `HintDrawResult.hovered`)를 `state.popup_hovered` / `state.banner_hovered` / `state.modifier_hint_hovered`(`src/state.rs`)에 프레임 간 상태로 저장해 두고(배너·modifier-hint 는 `src/adapters/ui/notification.rs` 의 draw 패스에서 기록), 마우스 핸들러가 그 값을 참조한다. (상세 모델은 [popup 시스템](../design/systems/popup.md) · [banner 시스템](../design/systems/banner.md).)

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
