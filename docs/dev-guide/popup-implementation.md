# Popup 구현 가이드

## Window / Modal / Popup 구분

이 세 개념은 명확히 구분된다. 코드에서 혼용하면 안 된다.

| 개념 | 정의 | 입력 차단 | 동시 열기 | 구현 |
|------|------|----------|----------|------|
| **Window** | 독립 OS 윈도우 | OS 네이티브 포커스 | 여러 개 가능 | `winit::window::Window` + sealed trait |
| **Modal** | Window의 특수 modality. 전역 입력 독점 | 닫기 전 다른 조작 불가 | 엔진 전역 최대 1개 | 별도 OS 윈도우 (SettingsWindow, QuitWindow 등) |
| **Popup** | Window 내부 가상 창 | 키보드: 포커스 시 차단. 마우스: 입력 계층에 따라 소비 | 여러 개 가능 | `PopupManager` + `PopupDef` |

상세 정의: `docs/design/ubiquitous-language.md`

## 핵심 규칙

**모든 내부 팝업(다이얼로그 포함)은 `PopupManager`/`PopupDef` 시스템으로 구현해야 한다.**

`egui::Window`를 직접 사용하면 안 되는 이유:
- `PopupManager`의 입력 계층(`popup_hovered`)을 우회한다
- 팝업 위를 클릭해도 뒤의 surface가 클릭을 받는다
- z-order, 드래그, 경계 제한 등 공통 기능이 적용되지 않는다

## 팝업 추가 방법

### 1. draw 함수 작성

`src/ui/my_popup.rs`에 draw 함수를 작성한다:

```rust
use crate::state::AppState;
use crate::ui::popup::PopupAction;

pub fn draw_my_popup(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    // Escape로 닫기
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    // 콘텐츠 렌더링...

    PopupAction::None
}
```

### 2. PopupDef 등록

`src/ui/popup_defs.rs`의 `all_defs()`에 항목을 추가한다:

```rust
PopupDef {
    id: "my_popup",
    title_key: "my_popup.title",           // i18n 키
    title_fn: None,                         // 동적 타이틀이 필요하면 Some(fn)
    default_size: egui::vec2(280.0, 120.0),
    sizer: None,                            // 동적 크기가 필요하면 Some(fn)
    default_scope: PopupScope::Window,
    close_on_outside_click: false,
    headless: false,                        // true면 타이틀바 없이 콘텐츠만
    sticky_focus: false,                    // true면 바깥 클릭해도 키보드 포커스 유지
    draw_fn: super::my_popup::draw_my_popup,
},
```

### 3. 팝업 열기

```rust
state.popups.open_centered_focused("my_popup");
```

### 4. 닫기 시 정리

`src/ui/notification.rs`의 `draw_popups()` 함수에 cleanup 코드를 추가한다:

```rust
let my_popup_closed = dispatch_closed.contains(&"my_popup")
    || draw_result.closed.contains(&"my_popup");
if my_popup_closed {
    state.dialogs.my_popup_data = None;
}
```

## PopupDef 필드 설명

| 필드 | 타입 | 설명 |
|------|------|------|
| `id` | `&'static str` | 고유 식별자 |
| `title_key` | `&'static str` | i18n 키. `t()`로 번역하여 타이틀바에 표시 |
| `title_fn` | `Option<fn(&AppState) -> String>` | 동적 타이틀. 설정 시 `title_key` 대신 사용. 대상에 따라 제목이 바뀌는 팝업에 사용 (예: rename popup) |
| `default_size` | `egui::Vec2` | 기본 크기. `TITLE_BAR_HEIGHT`(28px) + `CONTENT_MARGIN`(4px) 포함 |
| `sizer` | `Option<fn(&AppState) -> egui::Vec2>` | 동적 크기. open 시점에 1회 호출 |
| `default_scope` | `PopupScope` | 가시성/경계 범위 (Window, Workspace, Pane, Tab, Surface) |
| `close_on_outside_click` | `bool` | true면 팝업 바깥 클릭 시 닫힘 |
| `draw_fn` | `fn(&mut Ui, &mut AppState) -> PopupAction` | 매 프레임 호출되는 렌더링 함수 |

## 텍스트 입력이 있는 팝업

TextEdit를 사용하는 팝업에서는 다음 패턴을 따른다:

```rust
let resp = ui.add_sized([width, 22.0], egui::TextEdit::singleline(buffer));

// 포커스 자동 유지
if !resp.has_focus() {
    resp.request_focus();
}

// 첫 프레임에 전체 선택
if resp.gained_focus() {
    if let Some(mut text_state) = egui::TextEdit::load_state(ctx, resp.id) {
        let len = buffer.chars().count();
        text_state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(len),
        )));
        text_state.store(ctx, resp.id);
    }
}

// Enter로 확인
if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
    // apply
}
```

**주의**: `lost_focus()`만으로 팝업을 닫으면 안 된다. 팝업 내부의 다른 영역(버튼, 배경)을 클릭해도 TextEdit가 포커스를 잃기 때문이다. 반드시 Enter/Escape 키 또는 명시적 버튼 클릭으로만 닫아야 한다.

## 관련 설계 문서

- `docs/design/popup-system.md`: 팝업 시스템 전체 설계
- `docs/design/input-layer.md`: 마우스 입력 계층 (z-order와 입력 소비)
- `docs/design/ubiquitous-language.md`: Window/Modal/Popup 용어 정의
