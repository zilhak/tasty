# 컨텍스트 메뉴 구현 가이드

## 원칙

**모든 우클릭 컨텍스트 메뉴는 OS 네이티브 메뉴를 사용한다.** egui의 `Area`, `Window`, `menu` 등으로 자체 메뉴를 구현하면 안 된다.

이유:
- 네이티브 메뉴는 클릭 위치에 고정됨 (egui Area는 마우스를 따라다님)
- WebView 등 네이티브 자식 뷰 위에 올바르게 렌더링됨
- OS 일관성 (폰트, 애니메이션, 접근성)

## 구현 패턴

컨텍스트 메뉴는 2단계로 동작한다:

### 1단계: egui에서 우클릭 감지 → PendingNativeMenu 설정

egui render 루프 안에서는 OS 네이티브 메뉴를 직접 호출할 수 없다 (`winit::Window` 참조 불가). 대신 `PendingNativeMenu` enum에 필요한 정보와 클릭 좌표를 저장한다.

```rust
// state/mod.rs
pub enum PendingNativeMenu {
    Tab { pane_id: u32, tab_index: usize, x: f32, y: f32 },
    Pane { pane_id: u32, x: f32, y: f32 },
    ExplorerFolder { path: String, is_bookmarked: bool, x: f32, y: f32 },
    // 새 메뉴 유형 추가 시 여기에 variant 추가
}
```

egui 이벤트 핸들러에서:
```rust
if resp.secondary_clicked() {
    let pos = resp.interact_pointer_pos().unwrap_or_default();
    state.dialogs.pending_native_menu = Some(PendingNativeMenu::MyMenu {
        data: ...,
        x: pos.x,
        y: pos.y,
    });
}
```

### 2단계: MainView에서 네이티브 메뉴 표시 + 결과 처리

`window/main/redraw.rs`의 `process_pending_native_menu()`에서 `PendingNativeMenu`를 꺼내 OS 네이티브 메뉴를 표시하고, 선택 결과를 즉시 처리한다.

```rust
PendingNativeMenu::MyMenu { data, x, y } => {
    let items = [
        MenuItem::new(1, "Action A"),
        MenuItem::new(2, "Action B"),
    ];
    let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
    match result {
        Some(1) => { /* Action A */ }
        Some(2) => { /* Action B */ }
        _ => {}
    }
    self.mark_dirty();
}
```

## 네이티브 메뉴 API

`src/native_menu/mod.rs`에 크로스 플랫폼 인터페이스가 정의되어 있다.

| 함수 | 설명 |
|------|------|
| `show_context_menu(window, x, y, items)` | 지정 좌표에 네이티브 메뉴 표시. 선택된 항목의 ID를 반환 (`Option<u32>`) |
| `MenuItem::new(id, label)` | 일반 메뉴 항목 |
| `MenuItem::disabled(id, label)` | 비활성 (회색) 항목 |
| `MenuItem::separator()` | 구분선 |

## 새 컨텍스트 메뉴 추가 체크리스트

1. `PendingNativeMenu`에 새 variant 추가 (좌표 `x: f32, y: f32` 필수)
2. egui에서 `secondary_clicked()` 감지 → `pending_native_menu` 설정
3. `process_pending_native_menu()`에 match 분기 추가
4. 메뉴 항목 텍스트는 `crate::i18n::t()` 사용
5. `lang/en.toml`, `lang/ko.toml`, `lang/ja.toml`에 번역 키 추가
