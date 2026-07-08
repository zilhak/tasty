# 컨텍스트 메뉴 구현

**모든 우클릭 컨텍스트 메뉴는 OS 네이티브 메뉴를 쓴다.** egui `Area`/`Window`/`menu` 로 자체 구현 금지.

이유: 네이티브 메뉴는 클릭 위치 고정(egui Area 는 마우스 추종) · WebView 등 네이티브 자식 뷰 위에 올바르게 렌더 · OS 일관성(폰트/애니메이션/접근성).

## surface 컨텍스트 메뉴 생산자 (terminal vs 비-terminal)

surface 우클릭 컨텍스트 메뉴는 surface 종류에 따라 생산 경로가 갈린다:

- **terminal**: winit 경로(`src/view/main/mouse.rs` `handle_right_button`)가 생산한다. mouse-tracking 위임(ADR-0019/0022) 판정이 여기 있어 winit-level 이어야 한다.
- **비-terminal**(explorer/empty/markdown/image/webview/remote): winit 은 메뉴를 만들지 않고(`return`) **egui 프레임이 단일 생산자**다. `emit_surface_menu_fallback`(`src/adapters/ui/egui_panels.rs`)이 release 시점 `secondary_clicked()` 로 발화해 `PendingNativeMenu::Surface` 를 세팅한다. explorer 는 같은 프레임 안에서 `apply_explorer_action` 이 위치별 메뉴를 먼저 선점하고, fallback 은 `is_none()` 가드로 이를 존중한다.
- **explorer 표면 전체 커버리지**: explorer 는 위치별 메뉴(그리드 파일 셀 → 파일 메뉴, content 빈 영역 → Empty 메뉴, 트리 노드/즐겨찾기 → 각 메뉴)에 더해, `draw_explorer` 끝에서 **표면 전체 rect catch-all** 로 나머지 chrome(툴바/주소창/내부 탭바/상태줄/빈 사이드바)의 우클릭도 Empty 메뉴로 선점한다. 이로써 generic Surface fallback("터미널 ID 복사")이 explorer 위 어디에서도 뜨지 않는다(불가침 원칙 §1·§2 — 파일 브라우저에 무관한 surface-op 메뉴 노출 금지). 권한 거부 루트(`LoadState::NoPermission`)만 예외로, 붙여넣기가 무의미하므로 catch-all 을 건너뛴다.

이렇게 나눈 이유: 과거 winit(press 시점)과 egui(release 시점) 두 생산자가 같은 슬롯을 두고 경합했는데, 중앙 surface 위에서는 `egui_consumed` 가 구조적으로 항상 false 라 winit press 가 늘 먼저 generic Surface 메뉴를 선점 → explorer 위치별 메뉴가 실행되지 못했다. 비-terminal 생산을 egui release 단일 경로로 일원화해 이 경합을 제거했다.

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
