# 컨텍스트 메뉴 구현

**모든 우클릭 컨텍스트 메뉴는 OS 네이티브 메뉴를 쓴다.** egui `Area`/`Window`/`menu` 로 자체 구현 금지.

이유: 네이티브 메뉴는 클릭 위치 고정(egui Area 는 마우스 추종) · WebView 등 네이티브 자식 뷰 위에 올바르게 렌더 · OS 일관성(폰트/애니메이션/접근성).

## 2단계 패턴

egui render 루프 안에서는 OS 네이티브 메뉴를 직접 호출할 수 없다(`winit::Window` 참조 불가). 우클릭 시점에 정보+좌표만 저장하고, MainView 가 꺼내 표시한다.

### 1. egui 에서 우클릭 감지 → `PendingNativeMenu` 설정 (`src/state.rs`)

```rust
pub enum PendingNativeMenu {
    Tab { pane_id: u32, tab_index: usize, x: f32, y: f32 },
    Pane { pane_id: u32, x: f32, y: f32 },
    ExplorerFolder { path: String, is_bookmarked: bool, x: f32, y: f32 },
    // 새 메뉴 유형 → variant 추가 (좌표 x/y 필수)
}
```

```rust
if resp.secondary_clicked() {
    let pos = resp.interact_pointer_pos().unwrap_or_default();
    state.dialogs.pending_native_menu = Some(PendingNativeMenu::MyMenu { data, x: pos.x, y: pos.y });
}
```

### 2. MainView 에서 표시 + 결과 처리 (`src/view/main/redraw.rs`)

`process_pending_native_menu()` 가 `PendingNativeMenu` 를 꺼내 메뉴 표시 + 선택 즉시 처리:

```rust
PendingNativeMenu::MyMenu { data, x, y } => {
    let items = [MenuItem::new(1, "Action A"), MenuItem::new(2, "Action B")];
    match show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items) {
        Some(1) => { /* A */ }
        Some(2) => { /* B */ }
        _ => {}
    }
    self.mark_dirty();
}
```

## 네이티브 메뉴 API (`src/platform/native_menu/`)

| 함수 | 설명 |
|------|------|
| `show_context_menu(window, x, y, items)` | 좌표에 메뉴 표시 → 선택 ID(`Option<u32>`) |
| `MenuItem::new(id, label)` / `disabled(id, label)` / `separator()` | 항목 / 비활성 / 구분선 |

`macos.rs` / `windows.rs` / `linux.rs` 가 플랫폼 구현.

## 새 메뉴 체크리스트

1. `PendingNativeMenu` variant 추가(좌표 `x/y` 필수).
2. egui `secondary_clicked()` → `pending_native_menu` 설정.
3. `process_pending_native_menu()` 에 match 분기.
4. 항목 텍스트는 `crate::i18n::t()` + `lang/{en,ko,ja}.toml` 키 추가.
