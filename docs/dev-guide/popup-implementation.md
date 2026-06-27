# Popup 구현 가이드

View 내부 가상 창은 모두 **`PopupManager` + `PopupDef` 시스템**으로 만든다. `egui::Window` 를 직접 쓰지 않는다. 용어(Window/Modal/Popup/Toast 구분)는 [concepts/ubiquitous-language](../concepts/ubiquitous-language.md), 시스템 설계는 [`design/systems/popup.md`](../design/systems/popup.md).

> **0단계 — gallery-first**: 새 팝업은 본체에 넣기 **전에** 갤러리에 먼저 만든다(디자인 수령 → 갤러리 specimen → 본체). 아래 3단계는 그 "본체 반영" 단계다. 절차·근거는 [gallery-first](gallery-first.md) · [ADR-0020](../adr/0020-gallery-complete-component-source.md).

## 왜 `egui::Window` 직접 사용 금지

- `PopupManager` 의 입력 계층(`popup_hovered`)을 우회 → 팝업 위를 클릭해도 뒤 surface 가 클릭을 받는다.
- z-order·드래그·스코프 경계 클램핑 같은 공통 동작이 빠진다.

(예외: `src/gfx/gpu/shell_setup.rs` 의 부팅 전 셸 셋업처럼 popup 시스템이 살아있기 전 단계만. 앱 내부 다이얼로그는 전부 PopupDef.)

## 팝업 추가 — 3단계

### 1. draw 함수 (`src/adapters/ui/...`)

```rust
use crate::state::AppState;
use crate::adapters::ui::popup::PopupAction;

pub fn draw_my_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    core: &mut crate::core::CoreState,
) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;   // None | Close 둘뿐
    }
    // 콘텐츠 렌더...
    PopupAction::None
}
```

### 2. `PopupDef` 등록 (`src/adapters/ui/popup/defs.rs::all_defs()`)

`all_defs()` 의 `vec![...]` 에 한 항목 추가:

```rust
PopupDef {
    id: "my_popup",
    title_key: "my_popup.title",     // i18n 키 (t() 로 번역)
    title_fn: None,                  // 동적 제목이면 Some(fn(&AppState, &CoreState) -> String)
    default_size: egui::vec2(280.0, 120.0),
    sizer: None,                     // 동적 크기면 Some(fn(&AppState, &CoreState) -> Vec2)
    default_scope: PopupScope::Window,  // Window/Workspace/Pane/Tab/Surface
    close_on_outside_click: false,
    headless: false,                 // true = 타이틀바·닫기버튼 없이 콘텐츠만 (컨텍스트 메뉴 스타일)
    sticky_focus: false,             // true = 바깥 클릭해도 키보드 포커스 유지 (검색바 등)
    draw_fn: super::my_popup::draw_my_popup,
}
```

### 3. 팝업 열기 — Intent 큐로 발화

`state.popups.open*` 직접 호출 금지 — **Intent 로 발화**한다 (origin 정책·디스패치 이유는 [`design/flows/action-dispatch.md`](../design/flows/action-dispatch.md)).

```rust
use crate::intent::{UiIntent, OpenPopupMode};
state.dispatch_intent(UiIntent::OpenPopup { id: "my_popup", mode: OpenPopupMode::CenteredFocused }.from_user_menu("my_button"));
```

`OpenPopupMode`: `Default` · `CenteredFocused` · `WithScope(scope)` · `AtTopOfScope(scope)` · `AtFocused(pos)`. 발화 origin(`from_user_*` / `from_agent_*`)에 맞는 mode 를 고른다. 같은 id 가 이미 열려 있으면 두 번째 OpenPopup 은 dedup 으로 무시된다.

## `PopupDef` 필드

| 필드 | 타입 | 설명 |
|------|------|------|
| `id` | `PopupId`(`&'static str`) | 고유 식별자 |
| `title_key` | `&'static str` | i18n 키 → 타이틀바 |
| `title_fn` | `Option<fn(&AppState, &CoreState) -> String>` | 동적 제목. 설정 시 `title_key` 대신 매 프레임 호출 |
| `default_size` | `egui::Vec2` | 기본 크기 (unzoomed baseline) |
| `sizer` | `Option<fn(&AppState, &CoreState) -> Vec2>` | 동적 크기. **`ui_scale_factor()` 곱 금지** — sizing 토큰에 host UI zoom 이 이미 baked. 추가 곱은 이중 곱셈으로 medium/large 에서 layout 붕괴. **사용자가 직접 리사이즈한 팝업(`resizable`)에서는 리사이즈 이후 sizer 가 크기를 덮어쓰지 않는다**(`size_user_overridden` 가드 — popup close 시 리셋되어 다음 open 에 복원) |
| `default_scope` | `PopupScope` | 가시성/경계 범위 |
| `close_on_outside_click` | `bool` | 바깥 클릭 시 닫힘 |
| `headless` | `bool` | 타이틀바 없이 콘텐츠만 |
| `sticky_focus` | `bool` | 바깥 클릭해도 키보드 포커스 유지 |
| `drag_handle` | `DragHandle` | 이동(드래그) 핸들 선언. `None`(이동 불가) / `TitleBar`(타이틀바=핸들, 기존 동작; headless 면 핸들 없음) / `Region(fn(&PopupState)->Rect)`(팝업이 pos/size 로부터 **전용 핸들 띠** 계산 — 타이틀바 없는 팝업도 이동 가능). `movable` 여부는 별도 bool 없이 이 값으로 표현 |
| `resizable` | `bool` | true 면 테두리 8방향 드래그로 크기 조절(min_size·scope 경계 클램프, 엣지별 리사이즈 커서) |
| `min_size` | `Option<egui::Vec2>` | 리사이즈 최소 크기. `None`이면 `default_size`를 최소로 사용 |
| `draw_fn` | `fn(&mut Ui, &mut AppState, &mut CoreState) -> PopupAction` | 매 프레임 렌더 |

### 이동 / 리사이즈

- **이동**: `drag_handle` 으로 선언한 영역을 클릭+드래그 → 스코프 경계 안에서 위치 이동. 타이틀바 팝업은 `DragHandle::TitleBar`(기본). 타이틀바 없는 팝업은 `DragHandle::Region(fn)` 으로 pos/size 로부터 핸들 띠를 직접 계산해 선언한다.
  - **위젯 우선 중재(`is_using_pointer`)**: 이동/리사이즈의 *START 판정* 은 콘텐츠 렌더 **뒤** 에서 `ctx.is_using_pointer()` 게이트로 한다. 이번 프레임에 egui 위젯(버튼·입력)이 프레스를 가져갔으면 이동/리사이즈는 발동하지 않는다 → 핸들 띠가 위젯과 겹쳐도 **위젯이 항상 우선**(입력 우선순위: 위젯 > 리사이즈 > 이동). 따라서 `Region` 은 헤더 띠 전체처럼 넓은 영역을 가리켜도 안전하다(예: `port_scanner` 가 좁은 폭에서 좌측 띠와 검색 입력이 겹쳐도 입력 클릭이 우선). 단 **close 버튼은 매니저가 직접 페인팅** 한 영역이라 egui 위젯이 아니므로 `is_using_pointer` 에 안 잡힌다 → close 는 콘텐츠 렌더 *전* 에 따로 hit-test 해 우선 처리한다.
- **리사이즈**: `resizable: true` 팝업은 테두리 밴드(약 6px)를 잡아 8방향으로 크기 조절. 우선순위는 **close 버튼 > 리사이즈 엣지 > 드래그 핸들 > 콘텐츠**.

## 텍스트 입력이 있는 팝업

```rust
let resp = ui.add_sized([width, 22.0], egui::TextEdit::singleline(buffer));
if !resp.has_focus() { resp.request_focus(); }            // 포커스 자동 유지
if resp.gained_focus() { /* 첫 프레임 전체 선택 (TextEdit::load_state) */ }
if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { /* apply */ }
```

**주의**: `lost_focus()` 만으로 닫지 않는다 — 팝업 내 다른 영역 클릭에도 TextEdit 는 포커스를 잃는다. **Enter/Escape 또는 명시적 버튼**으로만 닫는다.

## 콘텐츠 레이어 — `egui::Area` 등록 (스크롤·클립)

팝업 콘텐츠(`draw_fn`)는 `PopupManager::draw`(`popup/draw.rs`)에서 **`egui::Area` 로 등록**되어 렌더된다. Area id = bg/title painter 와 동일한 layer_id(`Id("popup")+popup_id+z_idx`) → 한 레이어로 통합(z-order 자동 정합).

- **왜 Area 여야 하나**: egui 의 `Memory::layer_id_at` 은 **등록된 Area 만** 인식한다. 콘텐츠를 bare `Ui::new(layer_id)` 로 그리면 layer_id_at 이 팝업 레이어를 못 찾아 `ScrollArea::ui_contains_pointer()`=false → **휠/드래그 스크롤 입력이 무시**된다(위젯 클릭은 별도 widget hit-test 경로라 정상 → "클릭은 되는데 스크롤만 안 되는" 증상). 그래서 스크롤 가능한 콘텐츠(`ScrollArea`, `egui_extras::Table`)를 담는 팝업은 Area 등록이 필수다.
- **`movable(false)` + `sense(hover)`**: 드래그/클램핑/outside-click 은 `PopupManager` 가 **수동 좌표 hit-test**(`popup_hovered`)로 처리하므로 Area 가 클릭/드래그를 소비하지 않게 한다. egui Area 등록은 egui 내부 스크롤/호버 라우팅 전용이고, 터미널 입력 차단(`popup_hovered`, geometry 기반)과는 독립이다.
- **`set_min_size`/`set_max_size`(content_rect) + `set_clip_rect`(content_rect) 필수**: Area 는 콘텐츠에 맞춰 auto-shrink 하므로, footer 처럼 `allocate_new_ui` 로 별도 배치되는 요소가 빠지면 Area hit-rect 가 줄어 layer_id_at 이 팝업 하단을 못 잡는다 → `set_min_size` 로 hit-rect 를 content_rect 전체로 강제. 또 `egui::Ui::new(max_rect(r))` 는 clip_rect=r 였지만 Area 는 기본 clip 이 더 넓어 콘텐츠 넘침(긴 라벨·선택 하이라이트·스크롤바)이 팝업 밖으로 샌다 → `set_clip_rect(content_rect)` 로 경계 클립 복원.

> 즉 popup `draw_fn` 안에서는 일반 egui 위젯/`ScrollArea`/`Table` 을 그냥 쓰면 된다 — 스크롤·클립·레이어 등록은 `PopupManager::draw` 가 콘텐츠를 감싼 Area 가 처리한다.

## 닫힘 정리

팝업이 닫히면(`PopupManager::draw()` 의 `PopupDrawResult.closed` 에 id 포함) 그 팝업이 쥐고 있던 draft 버퍼/대상 상태를 함께 비운다. 안 비우면 reopen 시 이전 입력이 남는다.

## 관련

- [concepts/ubiquitous-language](../concepts/ubiquitous-language.md) — Window/Modal/Popup/Toast 구분
- [`design/systems/popup.md`](../design/systems/popup.md) — 팝업 시스템 전체 설계 (스코프·z-order·입력 계층)
- [architecture/input-layer](../architecture/input-layer.md) — 마우스 입력 계층/소비
