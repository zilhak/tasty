# Popup 구현 가이드

View 내부 가상 창은 모두 **`PopupManager` + `PopupDef` 시스템**으로 만든다. `egui::Window` 를 직접 쓰지 않는다. 용어(Window/Modal/Popup/Toast 구분)는 [concepts/ubiquitous-language](../concepts/ubiquitous-language.md), 시스템 설계는 [`design/systems/popup.md`](../design/systems/popup.md).

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

`state.popups.open*` 직접 호출 금지 — **Intent 로 발화**한다 (origin 정책·디스패치 이유는 `design/flows/action-dispatch.md` *재작성 예정*).

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
| `sizer` | `Option<fn(&AppState, &CoreState) -> Vec2>` | 동적 크기. **`ui_scale_factor()` 곱 금지** — sizing 토큰에 host UI zoom 이 이미 baked. 추가 곱은 이중 곱셈으로 medium/large 에서 layout 붕괴 |
| `default_scope` | `PopupScope` | 가시성/경계 범위 |
| `close_on_outside_click` | `bool` | 바깥 클릭 시 닫힘 |
| `headless` | `bool` | 타이틀바 없이 콘텐츠만 |
| `sticky_focus` | `bool` | 바깥 클릭해도 키보드 포커스 유지 |
| `draw_fn` | `fn(&mut Ui, &mut AppState, &mut CoreState) -> PopupAction` | 매 프레임 렌더 |

## 텍스트 입력이 있는 팝업

```rust
let resp = ui.add_sized([width, 22.0], egui::TextEdit::singleline(buffer));
if !resp.has_focus() { resp.request_focus(); }            // 포커스 자동 유지
if resp.gained_focus() { /* 첫 프레임 전체 선택 (TextEdit::load_state) */ }
if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { /* apply */ }
```

**주의**: `lost_focus()` 만으로 닫지 않는다 — 팝업 내 다른 영역 클릭에도 TextEdit 는 포커스를 잃는다. **Enter/Escape 또는 명시적 버튼**으로만 닫는다.

## 닫힘 정리

팝업이 닫히면(`PopupManager::draw()` 의 `PopupDrawResult.closed` 에 id 포함) 그 팝업이 쥐고 있던 draft 버퍼/대상 상태를 함께 비운다. 안 비우면 reopen 시 이전 입력이 남는다.

## 관련

- [concepts/ubiquitous-language](../concepts/ubiquitous-language.md) — Window/Modal/Popup/Toast 구분
- [`design/systems/popup.md`](../design/systems/popup.md) — 팝업 시스템 전체 설계 (스코프·z-order·입력 계층)
- `architecture/input-layer.md` *(재작성 예정)* — 마우스 입력 계층/소비
