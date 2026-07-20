# 컨텍스트 메뉴 구현

**모든 우클릭 컨텍스트 메뉴는 OS 네이티브 메뉴를 쓴다.** egui `Area`/`Window`/`menu` 로 자체 구현 금지.

이유: 네이티브 메뉴는 클릭 위치 고정(egui Area 는 마우스 추종) · WebView 등 네이티브 자식 뷰 위에 올바르게 렌더 · OS 일관성(폰트/애니메이션/접근성).

## surface 컨텍스트 메뉴 생산자 (terminal vs 비-terminal)

surface 우클릭 컨텍스트 메뉴는 surface 종류에 따라 생산 경로가 갈린다:

- **terminal**: winit 경로(`src/view/main/mouse.rs` `handle_right_button`)가 생산한다. mouse-tracking 위임(ADR-0019/0022) 판정이 여기 있어 winit-level 이어야 한다.
- **비-terminal**(explorer/empty/markdown/image/webview/remote): winit 은 메뉴를 만들지 않고(`return`) **egui 프레임이 단일 생산자**다. `emit_surface_menu_fallback`(`src/adapters/ui/egui_panels.rs`)이 release 시점 `secondary_clicked()` 로 발화해 `PendingNativeMenu::Surface` 를 세팅한다. explorer 는 같은 프레임 안에서 `apply_explorer_action` 이 위치별 메뉴를 먼저 선점하고, fallback 은 `is_none()` 가드로 이를 존중한다.
- **explorer 표면 전체 커버리지**: explorer 는 위치별 메뉴(그리드 파일 셀 → 파일 메뉴, content 빈 영역 → Empty 메뉴, 트리 노드/즐겨찾기 → 각 메뉴)에 더해, `draw_explorer` 끝에서 **표면 전체 rect catch-all** 로 나머지 chrome(툴바/주소창/내부 탭바/상태줄/빈 사이드바)의 우클릭도 Empty 메뉴로 선점한다. 이로써 generic Surface fallback("터미널 ID 복사")이 explorer 위 어디에서도 뜨지 않는다(불가침 원칙 §1·§2 — 파일 브라우저에 무관한 surface-op 메뉴 노출 금지). 권한 거부 루트(`LoadState::NoPermission`)만 예외로, 붙여넣기가 무의미하므로 catch-all 을 건너뛴다.
- **fallback 의 explorer-aware 최후 방어선**: catch-all 이 어떤 이유(런타임 이벤트 프레이밍·좌표 미스 등)로 explorer 슬롯을 못 세우고 `emit_surface_menu_fallback` 이 발화하더라도, fallback 은 **explorer surface 위에서는 generic `Surface` 대신 빈영역 `Explorer` 메뉴**(paths 빈 vec + cwd = current_root)를 세운다. 첫 패스에서 각 패널의 explorer 여부와 `current_root` 를 `EguiPanelInfo.explorer_cwd` 로 캡처해 판별한다. 그래서 explorer 위에는 어떤 경로로도 surface-op 메뉴가 노출되지 않고, 사용자는 항상 explorer 메뉴를 받는다. OS 무관 순수 로직(`#[cfg]` 불필요)이며, explorer 위 generic 메뉴는 원래 어느 OS 에서도 뜨면 안 되므로 macOS 정상 경로 회귀도 불가능하다.

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

### Linux 구현 (GTK 3, X11 only)

winit 이 소유한 창은 GTK 소유가 아니라 기본적으로 대응하는 `GdkWindow` 가 없다. `linux.rs` 는 winit 창의 raw X11 XID 를 `gdkx11::X11Window::foreign_new_for_display` 로 감싼 foreign `GdkWindow` 를 만들어 `menu.popup_at_rect()` 의 `rect_window` 로 넘긴다(`host_api/webview/linux.rs` 와 동일 패턴). 이게 없으면 `gtk_menu_popup_at_rect: assertion 'GDK_IS_WINDOW (rect_window)' failed` 로 팝업 자체가 무효화된다.

**바깥 클릭 dismiss**: 진짜 트리거 `GdkEvent`(winit → egui 로 이미 소비된 뒤라 합성 불가)가 없어 `trigger_event` 는 항상 `None` 이고, `popup_at_rect` 내부의 자체 grab 은 `Gtk-WARNING: no trigger event for menu popup` 와 함께 사실상 no-op 이다. 이를 보완하려고 `linux.rs` 가 팝업이 실제로 mapped 된 직후(`glib::idle_add_local_once` 로 `popup_at_rect` 호출 다음 tick 에 지연 실행 — widget 의 `map` 시그널 핸들러 안에서 하면 아직 서버에 안 떴을 수 있어 `GrabNotViewable` 로 실패한다) `gdk::Seat::grab()` 으로 직접 포인터/키보드를 잡는다.

- **raw Xlib grab(`XGrabPointer`/`XGrabKeyboard`) 이 아니라 `gdk::Seat::grab()` 을 쓰는 이유**: GDK3 의 X11 백엔드는 XInput2(XI2) 로만 이벤트를 받는다. raw core-protocol grab 으로 리다이렉트된 클릭은 X11 프로토콜 레벨에서는 메뉴 창으로 도착하지만 GDK 의 이벤트 소스가 XI2 이벤트만 인식하므로 `GdkEventButton` 으로 변환되지 않아 GTK 위젯 로직(activate, 우리가 추가한 바깥 클릭 감지 핸들러 모두)에 전혀 도달하지 못한다(raw grab 으로 실제 테스트해 확인된 사실).
- **`owner_events=true` 로 잡는 이유**: `false` 로 잡으면 메뉴 "안" 클릭까지 전부 grab 창(메뉴 자신)으로 강제 리다이렉트되면서 GTK 내부 hit-test 가 깨져 항목 클릭(activate)이 아예 씹힌다(실측 회귀 — Rename 클릭 시 다이얼로그가 안 뜸). `true` 로 두면 자신이 소유한 서브윈도우 위의 클릭은 정상 라우팅되어 항목 클릭이 그대로 동작하고, 소유하지 않은 다른 창(tasty 메인 창 등) 위의 클릭만 grab 창으로 리다이렉트된다.
- 리다이렉트된 바깥 클릭은 `menu.connect_button_press_event` 핸들러가 좌표를 메뉴 자신의 allocation 과 비교해 밖이면 직접 `popdown()` 한다 — GTK 의 기본 deactivate 로직은 (트리거 이벤트 없이 잡은) grab 소유 여부를 스스로 신뢰하지 못해 기대할 수 없다.
- `Seat::grab` 이 실패(`GrabStatus::Success` 가 아님)해도 패닉하지 않고 경고만 남긴 뒤 grab 없이 진행한다 — 이 경우 바깥 클릭 dismiss 만 느려질 뿐(30초 워치독으로 폴백) 메뉴 자체나 항목 클릭은 영향받지 않는다.
- 루프 종료(선택/취소/타임아웃) 후 `grabbed` 가 true 였으면 반드시 `seat.ungrab()` 으로 대칭 해제한다.

무한 멈춤(grab 이 전혀 안 잡혀 `gtk_main_iteration_do` 루프가 영원히 안 빠지는 경우) 방지용 최후 안전장치로, `selection-done` 이 30초 안에 안 오면 강제로 `popdown()` 하는 워치독은 grab 성공 여부와 무관하게 항상 유지한다.

## 새 메뉴 체크리스트

1. `PendingNativeMenu` variant 추가(좌표 `x/y` 필수).
2. egui `secondary_clicked()` → `pending_native_menu` 설정.
3. `process_pending_native_menu()` 에 match 분기.
4. 항목 텍스트는 `crate::i18n::t()` + `lang/{en,ko,ja}.toml` 키 추가.
