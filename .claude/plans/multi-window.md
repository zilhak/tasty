# 구현 계획: 멀티 윈도우 아키텍처

설계 문서: `docs/design/multi-window-architecture.md`, `docs/design/focus-policy.md`, `docs/design/ubiquitous-language.md`

## 현재 구조 분석

### 문제: `App` 구조체가 엔진 + 윈도우 역할을 모두 담당

```
App (main.rs:84)
├── gpu: GpuState          ← 윈도우 고유 (wgpu surface, egui)
├── state: AppState        ← 엔진 고유 (워크스페이스, 설정, 알림, 훅)
├── window: Arc<Window>    ← 윈도우 고유
├── ipc_server: IpcServer  ← 엔진 고유
├── clipboard: Clipboard   ← 윈도우 고유 (OS 윈도우별)
├── dirty: bool            ← 윈도우 고유
├── modifiers: Modifiers   ← 윈도우 고유
├── preedit_text: String   ← 윈도우 고유
├── proxy: EventLoopProxy  ← 엔진 고유 (하나의 이벤트 루프)
└── port_file: String      ← 엔진 고유
```

### 문제: `AppState`도 엔진 상태 + UI 상태가 혼재

```
AppState (state.rs:69)
├── 엔진 상태 (공유해야 함)
│   ├── workspaces: Vec<Workspace>
│   ├── settings: Settings
│   ├── notifications: NotificationStore
│   ├── hook_manager: HookManager
│   ├── global_hook_manager: GlobalHookManager
│   ├── claude_*: HashMap (부모-자식 관계, idle 상태)
│   ├── surface_messages: HashMap
│   ├── last_key_input: HashMap
│   └── waker: Waker
│
├── UI 상태 (윈도우별로 독립)
│   ├── notification_panel_open: bool
│   ├── settings_open: bool
│   ├── settings_ui_state: SettingsUiState
│   ├── ws_rename: Option<(...)>
│   ├── pane_context_menu: Option<PaneContextMenu>
│   └── markdown_path_dialog: Option<(...)>
│
└── 레이아웃 상태 (윈도우별 또는 공유 — 결정 필요)
    ├── active_workspace: usize
    ├── sidebar_width: f32
    ├── default_cols/rows: usize
    └── next_ids: IdGenerator
```

### 문제: `event_handler.rs`가 단일 윈도우 전제

- `WindowEvent`에서 `WindowId`를 무시하고 `self.window`를 직접 참조
- `RedrawRequested`에서 하나의 GPU로 렌더링

### 문제: `gpu.rs::render()`가 너무 많은 역할

928줄. 셸 설정 모달, egui UI 전체, 터미널 렌더링, 스크린샷, 테마 적용, 폰트 리프레시를 모두 담당.

---

## Phase 0: AppState 분리 (선행 리팩토링)

멀티 윈도우 전에 먼저 상태 분리. 이것이 가장 중요한 단계.

### 0-1. `EngineState` 추출

`AppState`에서 엔진(공유) 상태를 `EngineState`로 분리.

**새 파일: `src/engine_state.rs`**

```rust
pub struct EngineState {
    // 워크스페이스/터미널 관리
    pub workspaces: Vec<Workspace>,
    pub next_ids: IdGenerator,
    pub default_cols: usize,
    pub default_rows: usize,
    pub waker: Waker,

    // 설정
    pub settings: Settings,

    // 알림/훅
    pub notifications: NotificationStore,
    pub hook_manager: HookManager,
    pub global_hook_manager: GlobalHookManager,

    // Claude 관련
    pub claude_parent_children: HashMap<u32, Vec<ClaudeChildEntry>>,
    pub claude_child_parent: HashMap<u32, u32>,
    pub claude_closed_parents: HashSet<u32>,
    claude_next_child_index: HashMap<u32, u32>,
    pub claude_idle_state: HashMap<u32, bool>,
    pub claude_needs_input_state: HashMap<u32, bool>,

    // 메시지/타이핑
    pub surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    surface_next_message_id: u32,
    pub last_key_input: HashMap<u32, std::time::Instant>,
}
```

**수정: `src/state.rs` → `WindowState` (윈도우별 UI 상태)**

```rust
pub struct WindowState {
    pub active_workspace: usize,
    pub sidebar_width: f32,
    pub notification_panel_open: bool,
    pub settings_open: bool,
    pub settings_ui_state: SettingsUiState,
    pub ws_rename: Option<(usize, WsRenameField, String)>,
    pub pane_context_menu: Option<PaneContextMenu>,
    pub markdown_path_dialog: Option<(u32, String)>,
}
```

**주의사항:**
- `EngineState`의 메서드 중 워크스페이스 조작(add_workspace, split_pane 등)은 그대로 `EngineState`에 유지
- `WindowState`는 해당 윈도우에서 어떤 워크스페이스를 보고 있는지(`active_workspace`)만 보유
- 기존 `AppState`를 사용하던 모든 코드를 `EngineState` + `WindowState`로 분리

### 0-2. 영향 받는 파일 목록

| 파일 | 변경 내용 |
|------|----------|
| `state.rs` | `AppState` → `EngineState` + `WindowState` 분리. 기존 메서드 재배치 |
| `main.rs` | `App`이 `EngineState` + `WindowState`를 별도로 소유 |
| `gpu.rs` | `render()`가 `EngineState`, `WindowState` 둘 다 받음 |
| `event_handler.rs` | 이벤트 핸들러가 둘 다 접근 |
| `ui.rs` | `draw_*` 함수들이 `EngineState` + `WindowState` 분리 접근 |
| `settings_ui.rs` | `settings`는 `EngineState`에서, `settings_open`은 `WindowState`에서 |
| `shortcuts.rs` | `handle_shortcut`이 둘 다 접근 |
| `ipc/handler/mod.rs` | IPC 핸들러는 `EngineState`만 접근 (UI 상태 불필요) |
| `ipc/handler/surface.rs` | 동일 |
| `ipc/handler/hooks.rs` | 동일 |

### 0-3. `gpu.rs::render()` 분해

현재 928줄의 `render()`를 역할별로 분리:

| 새 함수 | 역할 | 줄 수 (예상) |
|---------|------|-------------|
| `render()` | 오케스트레이션 (호출 순서만 관리) | ~50 |
| `prepare_layout()` | pane_rects, dividers 계산 | ~40 |
| `run_egui_frame()` | egui 프레임 실행 (UI 그리기) | ~80 |
| `render_terminals()` | 터미널 셀 렌더링 | ~60 |
| `render_egui()` | egui 결과를 GPU에 제출 | ~30 |
| `post_render()` | 스크린샷, 테마/폰트 리프레시 | ~40 |

### 0-4. `event_handler.rs` 분해

현재 643줄. `window_event` 함수 하나에 모든 이벤트가 들어있음.

| 새 함수/모듈 | 역할 |
|-------------|------|
| `handle_keyboard()` | `KeyboardInput` 이벤트 처리 |
| `handle_mouse()` | `CursorMoved`, `MouseInput`, `MouseWheel` 처리 |
| `handle_ime()` | `Ime` 이벤트 처리 |
| `handle_resize()` | `Resized` 이벤트 처리 |
| `handle_redraw()` | `RedrawRequested` — 렌더링 트리거 |

---

## Phase 1: 엔진/윈도우 구조체 분리

Phase 0 완료 후 실행.

### 1-1. `Engine` 구조체

**새 파일: `src/engine.rs`**

```rust
pub struct Engine {
    pub state: EngineState,
    pub ipc_server: Option<IpcServer>,
    pub proxy: EventLoopProxy<AppEvent>,
    pub modal_active: AtomicBool,
    port_file: Option<String>,
}
```

### 1-2. `TastyWindow` 구조체로 App 윈도우 필드 이동

**현재 상태**: `TastyWindow` 구조체는 정의됨 (`src/tasty_window.rs`). App은 아직 기존 필드(gpu, state, window 등)을 직접 소유.

**접근 방식**: `event_handler.rs`의 `window_event()`를 `TastyWindow`의 메서드로 이동. `App`의 `ApplicationHandler` impl은 `WindowId`로 `TastyWindow`를 찾아 위임만 하도록 변경. Rust borrow checker 문제로 `self.field` → `w.field` 단순 치환이 불가능하므로, `TastyWindow`에 `handle_event(&mut self, engine: &mut Engine, event: WindowEvent)` 패턴으로 구현.

**구체적 단계:**
1. `TastyWindow`에 `handle_window_event()`, `handle_keyboard_input()` 등 메서드를 이동
2. `shortcuts.rs`의 `handle_shortcut()`을 `TastyWindow`의 메서드로 이동
3. `App::window_event()`는 `self.primary_window.handle_event(&mut self.engine, event)`만 호출
4. 기존 `App`에서 gpu, state, window 등 윈도우 필드 제거

**새 파일: `src/tasty_window.rs` (이미 존재, 확장 필요)**

```rust
pub struct TastyWindow {
    pub gpu: GpuState,
    pub state: AppState,  // EngineState + WindowState 포함
    pub window: Arc<Window>,
    pub dirty: bool,
    pub modifiers: ModifiersState,
    pub window_focused: bool,
    pub cursor_position: Option<PhysicalPosition<f64>>,
    pub dragging_divider: Option<DividerDrag>,
    pub clipboard: Option<ClipboardContext>,
    pub preedit_text: String,
}
```

### 1-3. `App` 리팩토링

```rust
struct App {
    engine: Engine,
    windows: HashMap<WindowId, TastyWindow>,
    // 셸 설정 모드는 별도 처리 (첫 윈도우 생성 전)
    shell_setup_mode: bool,
    shell_setup_path: String,
}
```

### 1-4. 이벤트 루프 수정

`window_event`에서 `WindowId`로 해당 `TastyWindow`를 찾아 처리:

```rust
fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
    let window = match self.windows.get_mut(&id) {
        Some(w) => w,
        None => return,
    };
    // window + engine.state 를 함께 전달
    window.handle_event(&mut self.engine, event_loop, event);
}
```

---

## Phase 2: 멀티 윈도우 지원

### 2-1. 윈도우 생성

- `Engine::create_window()` → 새 OS 윈도우 + wgpu 서피스 + egui 초기화
- wgpu adapter/device는 `Engine`이 소유, 윈도우는 surface만 생성
- IPC 메서드 `window.create` 추가
- CLI `tasty new-window` 추가

### 2-2. 윈도우 파괴

- 마지막 윈도우 닫히면 앱 종료
- `Engine`에서 `TastyWindow` 제거
- IPC `window.close`, `window.list` 추가

### 2-3. wgpu 리소스 공유

현재 `GpuState`가 `device`, `queue`, `adapter`를 소유. 이것을 `Engine`으로 올림:

```rust
pub struct Engine {
    // GPU 공유 리소스
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub adapter: wgpu::Adapter,
    ...
}

pub struct TastyWindow {
    // 윈도우별 GPU 리소스
    pub surface: wgpu::Surface,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub renderer: CellRenderer,
    pub egui_renderer: egui_wgpu::Renderer,
    ...
}
```

---

## Phase 3: 모달 시스템

### 3-1. 설정창을 독립 윈도우로

- 설정 단축키 → `Engine`이 모달 윈도우 생성
- `engine.modal_active = true`
- 모달 윈도우는 `CellRenderer` 불필요 (egui만 사용)

### 3-2. 포커스 차단

각 `TastyWindow::handle_event()`에서:

```rust
if engine.modal_active.load(Ordering::Relaxed) {
    // 키보드/마우스 이벤트 무시
    return;
}
```

---

## Phase 4: IPC 확장

### 4-1. 윈도우 관련 IPC

| 메서드 | 설명 |
|--------|------|
| `window.list` | 윈도우 목록 |
| `window.create` | 새 윈도우 생성 |
| `window.close` | 윈도우 닫기 |
| `window.focus` | 윈도우 포커스 |

### 4-2. 기존 IPC에 `window_id` 파라미터

기존 메서드들은 `window_id`를 선택적 파라미터로 추가. 생략 시 포커스된 윈도우.

---

## 실행 순서

1. **Phase 0-1**: `EngineState` 추출 (가장 먼저, 가장 중요)
2. **Phase 0-2**: 영향 받는 파일 수정 (빌드 통과까지)
3. **Phase 0-3**: `gpu.rs::render()` 분해
4. **Phase 0-4**: `event_handler.rs` 분해
5. **Phase 1**: Engine/TastyWindow 구조체 분리
6. **Phase 2**: 멀티 윈도우 생성/파괴
7. **Phase 3**: 모달 시스템
8. **Phase 4**: IPC 확장

**각 Phase는 독립적으로 빌드 가능해야 한다.** Phase 0만 완료해도 기존 기능이 동작해야 한다.

---

## 주의사항

### 관심사 분리 원칙

- **하나의 함수는 하나의 역할**: `render()`가 UI + 렌더링 + 스크린샷 + 테마 리프레시를 하지 않도록
- **하나의 구조체는 하나의 소유권 범위**: 엔진 상태와 윈도우 상태를 같은 구조체에 넣지 않음
- **IPC 핸들러는 UI 상태를 몰라야 함**: IPC는 `EngineState`만 접근, 어떤 윈도우가 뭘 보여주는지는 관여 안 함

### 빌드 안정성

- Phase 0의 각 단계마다 `cargo build` 통과 확인
- 기존 테스트(`tests/`) 통과 확인
- 헤드리스 모드가 깨지지 않는지 확인 (헤드리스는 `EngineState`만 사용)

### 크로스플랫폼

- `Engine`의 wgpu adapter/device 생성은 플랫폼 독립
- `TastyWindow`의 surface 생성만 플랫폼별 (winit이 처리)
- 모달의 포커스 차단은 앱 레벨 (`AtomicBool`)
