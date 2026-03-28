# 멀티 윈도우 구현 계획 — 성능/문제점 분석 및 대안

`multi-window.md`의 각 Phase에 대한 분석.

---

## Phase 0: AppState 분리

### 0-1. EngineState 추출

**성능 오버헤드**: 없음. 구조체 분리는 런타임 비용이 없다. 메모리 레이아웃만 달라지며 컴파일러가 동일하게 최적화한다.

**문제점**: `EngineState`와 `WindowState`를 동시에 빌려야 하는 함수가 많다. 현재 `&mut AppState` 하나로 모든 걸 했는데, 분리하면 `(&mut EngineState, &mut WindowState)` 두 개를 전달해야 한다.

- `ui.rs`의 `draw_ui()`: 사이드바는 `EngineState`(워크스페이스 목록), 현재 뷰는 `WindowState`(active_workspace)
- `shortcuts.rs`의 `handle_shortcut()`: 워크스페이스 전환은 `WindowState`, 탭 생성은 `EngineState`
- `gpu.rs`의 `render()`: 양쪽 모두 필요

**대안 검토**:

| 접근 | 장점 | 단점 |
|------|------|------|
| A. 두 개의 `&mut` 파라미터 전달 | 가장 단순, Rust borrow checker 호환 | 함수 시그니처가 길어짐 |
| B. `AppContext` 래퍼 (`engine: &mut EngineState, window: &mut WindowState`) | 시그니처 간결 | 추가 구조체이지만 실질 비용 없음 |
| C. `EngineState`를 `Arc<Mutex<>>` | 멀티 윈도우 시 스레드 안전 | 단일 윈도우에서는 불필요한 lock 비용 |

**권장**: 현재 Phase 0에서는 **B**. 멀티 윈도우(Phase 1+)에서 `EngineState`를 `Arc<Mutex<>>`로 전환할 때 `AppContext` 래퍼 안에서만 변경하면 외부 인터페이스가 안정적.

```rust
pub struct AppContext<'a> {
    pub engine: &'a mut EngineState,
    pub window: &'a mut WindowState,
}
```

### 0-2. 영향 받는 파일 수정

**문제점**: 10개 파일을 동시에 수정해야 빌드가 통과한다. 중간 상태에서 빌드 불가능한 구간이 길다.

**대안**: 점진적 마이그레이션.

1. `EngineState`를 만들되, `AppState` 안에 포함시킨다 (composition)
2. `AppState`에 `engine()` / `window()` 접근자를 추가
3. 기존 코드를 한 파일씩 접근자 사용으로 전환
4. 전체 전환 완료 후 `AppState`를 해체

이렇게 하면 매 단계마다 빌드가 통과한다.

```rust
// 단계 1: AppState 내부에 EngineState를 포함
pub struct AppState {
    pub engine: EngineState,  // 기존 필드들을 여기로 이동
    // UI 상태는 그대로 AppState에 유지 (일단)
    pub notification_panel_open: bool,
    ...
}

// 단계 2: 접근자 제공
impl AppState {
    pub fn engine(&self) -> &EngineState { &self.engine }
    pub fn engine_mut(&mut self) -> &mut EngineState { &mut self.engine }
}

// 단계 3: 기존 코드를 점진적으로 state.workspaces → state.engine().workspaces로 전환
// 단계 4: 전체 완료 후 AppState 해체 → EngineState + WindowState
```

### 0-3. gpu.rs::render() 분해

**성능 오버헤드**: 없음. 함수 호출 비용은 인라인 최적화로 제거된다. `#[inline]`을 명시하지 않아도 같은 크레이트 내 함수는 LTO 시 자동 인라인.

**문제점**: `render()` 내에서 `self`의 여러 필드를 동시에 빌려야 하는 구간이 있다. 예를 들어 egui 클로저 안에서 `self.renderer`에 접근 불가 (이미 `cell_w`/`cell_h`를 미리 빼는 식으로 우회 중).

**대안**: 함수 분해 시 `self`를 통째로 전달하지 않고, 필요한 필드만 개별 파라미터로 전달하면 borrow 충돌을 피할 수 있다. 하지만 파라미터가 많아지므로, 관련 필드를 묶은 하위 구조체를 만드는 게 나을 수 있다.

```rust
// 현재: GpuState가 모든 것을 소유
pub struct GpuState {
    surface, device, queue, config, size,
    renderer, egui_ctx, egui_state, egui_renderer,
    scale_factor, pending_screenshot,
}

// 제안: 역할별 하위 그룹
pub struct GpuState {
    pub core: GpuCore,       // surface, device, queue, config, size
    pub renderer: CellRenderer,
    pub egui: EguiState,     // egui_ctx, egui_state, egui_renderer
    pub scale_factor: f32,
    pub pending_screenshot: Option<PathBuf>,
}
```

이러면 `self.core`와 `self.renderer`와 `self.egui`를 동시에 `&mut`로 빌릴 수 있다.

### 0-4. event_handler.rs 분해

**성능 오버헤드**: 없음. match 분기에서 함수 호출로 바뀔 뿐.

**문제점**: `handle_keyboard()`가 `self.clipboard`, `self.state`, `self.gpu`, `self.modifiers`를 모두 접근해야 함. `self`를 통째로 전달하면 분해 의미가 줄어듦.

**대안**: 이벤트 핸들러 함수는 `&mut self`를 그대로 받되, 내부 로직을 분류하는 역할만 수행. 실제 동작은 `App`의 별도 메서드로 분리.

```rust
// event_handler.rs
WindowEvent::KeyboardInput { event, .. } => self.handle_keyboard(event, egui_consumed),
WindowEvent::Ime(ime) => self.handle_ime(ime, egui_consumed),
WindowEvent::CursorMoved { position, .. } => self.handle_cursor_moved(position, egui_consumed),

// App impl (keyboard.rs, mouse.rs 등으로 분리 가능)
impl App {
    fn handle_keyboard(&mut self, event: KeyEvent, egui_consumed: bool) { ... }
    fn handle_ime(&mut self, event: Ime, egui_consumed: bool) { ... }
}
```

---

## Phase 1: 엔진/윈도우 구조체 분리

### 1-1~1-2. Engine + TastyWindow

**성능 오버헤드**: `EngineState`를 `Arc<Mutex<>>`로 감싸야 하면 lock 비용 발생.

**문제점 분석**: winit의 `ApplicationHandler`는 `&mut self`로 호출된다. 이벤트 루프는 단일 스레드. 따라서 Phase 1에서 `EngineState`를 `Arc<Mutex<>>`로 감쌀 필요가 있는지?

- **IPC 스레드**가 `EngineState`에 접근해야 함 → 현재는 채널(`mpsc`)로 메인 스레드에 전달 후 처리
- **PTY 읽기 스레드**는 `EngineState`에 직접 접근하지 않음 → waker로 이벤트만 전달

결론: **현재 구조에서는 `Arc<Mutex<>>` 불필요.** IPC는 채널을 통해 메인 스레드에서 처리하므로, `EngineState`는 메인 스레드 단독 소유. 멀티 윈도우여도 winit 이벤트 루프가 단일 스레드이므로 동일.

```rust
// Arc<Mutex<>> 없이 동작하는 구조
struct App {
    engine: Engine,  // 소유권: App (메인 스레드)
    windows: HashMap<WindowId, TastyWindow>,
}

// window_event에서:
fn window_event(&mut self, ..., id: WindowId, event: WindowEvent) {
    if let Some(window) = self.windows.get_mut(&id) {
        window.handle_event(&mut self.engine, event);
    }
}
```

`&mut self.engine`과 `&mut self.windows[&id]`는 서로 다른 필드이므로 Rust borrow checker가 허용한다.

**이것이 더 나은 이유**: lock 비용 제로, 데드락 불가능, 코드 단순.

### 1-3~1-4. App 리팩토링 + 이벤트 루프

**문제점**: `HashMap<WindowId, TastyWindow>`에서 윈도우를 꺼내면서 동시에 `engine`에 접근하는 패턴이 빈번. Rust의 borrow checker에서 `self.windows.get_mut()` + `&mut self.engine`은 허용되지만, 두 윈도우를 동시에 `&mut`로 꺼내는 건 불가.

**대안**: 윈도우 간 직접 통신이 필요하면 `Engine`을 중개자로 사용. 두 윈도우를 동시에 수정해야 하는 상황은 설계상 없어야 한다 (각 이벤트는 하나의 WindowId에 귀속).

---

## Phase 2: 멀티 윈도우 지원

### 2-3. wgpu 리소스 공유

**성능 오버헤드**: `Arc<wgpu::Device>`를 사용하면 reference counting 비용이 있지만, device/queue 접근은 프레임당 몇 번이므로 무시 가능.

**문제점**: wgpu `device.create_surface()`는 device가 특정 adapter에 바인딩되어 있고, adapter는 특정 surface와 호환성 체크를 한다. 멀티 모니터 환경에서 서로 다른 GPU에 연결된 모니터가 있으면, 하나의 adapter로 모든 surface를 지원하지 못할 수 있다.

**대안**:

| 접근 | 설명 |
|------|------|
| A. 단일 adapter/device, 모든 윈도우 공유 | 단순. 대부분의 환경에서 동작. 멀티 GPU 환경에서 실패 가능 |
| B. 윈도우 생성 시 호환성 체크, 필요하면 별도 adapter | 안전하지만 복잡 |
| C. 항상 power_preference=LowPower로 통합 GPU 사용 | 멀티 GPU 문제 회피, 성능 손해 가능 |

**권장**: **A**로 시작. 멀티 GPU 환경에서 surface 생성 실패 시 에러 로그 + 해당 윈도우만 fallback(egui만 사용하거나 생성 거부). 실제로 멀티 GPU 데스크톱은 극소수.

---

## Phase 3: 모달 시스템

### 3-1. 설정창 독립 윈도우

**성능 오버헤드**: 모달 윈도우는 `CellRenderer`(글리프 아틀라스, 터미널 렌더링 파이프라인)가 불필요. egui만 사용하면 GPU 메모리 절약.

**문제점**: 모달 윈도우에는 어떤 `GpuState`를 쓸 것인가?

| 접근 | 설명 |
|------|------|
| A. 전체 GpuState (CellRenderer 포함) | 간단하지만 불필요한 GPU 메모리 사용 |
| B. 경량 GpuState (egui만) | CellRenderer 없이 surface + egui_renderer만 |
| C. 모달 전용 구조체 `ModalWindow` | TastyWindow와 별도 타입, egui 전용 |

**권장**: **C**. 모달은 터미널 윈도우와 근본적으로 다르므로 별도 타입이 자연스럽다.

```rust
enum WindowKind {
    Terminal(TastyWindow),  // 터미널 렌더링 + egui
    Modal(ModalWindow),     // egui 전용
}
```

`App.windows: HashMap<WindowId, WindowKind>`로 관리하면 타입 안전.

### 3-2. 포커스 차단

**성능 오버헤드**: `AtomicBool::load(Relaxed)` 한 번 = 거의 0. 매 이벤트마다 호출해도 무시 가능.

**문제점**: `modal_active`가 `true`인데 모달 윈도우가 크래시하면? 포커스가 영원히 차단될 수 있다.

**대안**: 모달 윈도우의 `Drop` impl에서 `modal_active = false`로 설정. 또는 `modal_active` 대신 `modal_window_id: Option<WindowId>`를 사용해서, 해당 WindowId가 `windows` HashMap에 없으면 자동 해제.

```rust
// AtomicBool 대신
pub modal_window_id: Option<WindowId>,

// 포커스 차단 체크
fn is_modal_active(&self) -> bool {
    self.engine.modal_window_id
        .map(|id| self.windows.contains_key(&id))
        .unwrap_or(false)
}
```

이렇게 하면 모달 윈도우가 예기치 않게 닫혀도 자동 복구된다.

---

## Phase 4: IPC 확장

### 4-2. 기존 IPC에 window_id 파라미터

**문제점**: IPC 핸들러는 `EngineState`만 접근하는데, `window_id`를 처리하려면 윈도우 목록에도 접근해야 한다. 현재 IPC 채널 구조(`mpsc`)에서는 메인 스레드가 처리하므로 가능하지만, IPC 핸들러의 인터페이스가 달라져야 한다.

**대안**: IPC 핸들러를 두 계층으로 분리.

```
1. Engine-level handler: EngineState만 접근 (워크스페이스, 서피스, 훅 등)
2. App-level handler: windows + engine 접근 (window.list, window.create, ui.screenshot 등)
```

현재 `ui.screenshot`이 이미 App 레벨에서 특별 처리되고 있으므로, 이 패턴을 정식화.

---

## 전체 성능 요약

| Phase | 런타임 오버헤드 | 메모리 오버헤드 |
|-------|----------------|----------------|
| 0: AppState 분리 | 0 | 0 |
| 0: render() 분해 | 0 (인라인 최적화) | 0 |
| 0: event_handler 분해 | 0 | 0 |
| 1: Engine/TastyWindow 분리 | 0 (Arc<Mutex> 불필요) | 0 |
| 2: 멀티 윈도우 | 추가 윈도우당 wgpu surface + egui 초기화 비용 | 윈도우당 ~10-20MB (글리프 아틀라스 등) |
| 3: 모달 | 모달 전용 경량 구조체 사용 시 최소 | egui만 사용 시 ~2-5MB |
| 4: IPC 확장 | 0 (기존 채널 구조 재사용) | 0 |

**결론**: Phase 0~1은 순수 리팩토링으로 성능 영향 제로. Phase 2부터 추가 윈도우당 GPU 리소스가 소모되지만 현대 시스템에서 무시 가능한 수준.

---

## 권장 사항 요약

| 항목 | 구현 계획 원안 | 권장 변경 |
|------|---------------|----------|
| EngineState 접근 | 두 개의 `&mut` 파라미터 | `AppContext` 래퍼 사용 |
| AppState 마이그레이션 | 한 번에 분리 | 점진적 마이그레이션 (composition → 접근자 → 해체) |
| GpuState 분해 | 단일 구조체 유지 | 하위 그룹 (GpuCore, EguiState) 도입 |
| EngineState 동기화 | Arc<Mutex<>> | **불필요** — winit 이벤트 루프가 단일 스레드 |
| wgpu 리소스 공유 | 단일 adapter/device | 단일로 시작, 멀티 GPU는 fallback 처리 |
| 모달 윈도우 타입 | TastyWindow 재사용 | 별도 `ModalWindow` 타입 (egui 전용) |
| modal_active | AtomicBool | `Option<WindowId>` (자동 복구) |
| IPC 핸들러 | EngineState만 | Engine-level + App-level 2계층 분리 |
