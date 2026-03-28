# 구현 계획: 멀티 윈도우 아키텍처

설계 문서: `docs/design/multi-window-architecture.md`, `docs/design/focus-policy.md`, `docs/design/ubiquitous-language.md`
분석 문서: `.claude/plans/multi-window-analysis.md`

## 현재 상태 (Phase 0 + Phase 1 부분 완료)

```
App
├── engine: Engine (IPC 서버, proxy, modal_window_id)
├── gpu, state, window, dirty, modifiers... (윈도우 필드 — 아직 App에 직접 소유)
└── shell_setup_mode/path
```

```
AppState
├── engine: EngineState (워크스페이스, 설정, 알림, 훅, Claude, 메시지)
└── UI 필드 (active_workspace, settings_open 등)
```

**완료:**
- EngineState 추출 + AppState에 composition
- gpu.rs render() 7개 함수 분해
- event_handler.rs 키보드/IME/마우스 메서드 분리
- Engine 구조체 (IPC, proxy, modal)
- TastyWindow 구조체 정의 (미사용)

---

## Phase 1-2: TastyWindow로 윈도우 필드 이동

**핵심 작업.** App의 윈도우 필드(gpu, state, window, dirty 등)를 TastyWindow로 이동.

### 접근 방식

`event_handler.rs`의 이벤트 처리 로직을 `TastyWindow`의 메서드로 이동한다. `App::window_event()`는 TastyWindow를 찾아 위임만 한다. `Engine`은 별도 `&mut`로 전달하여 borrow checker 충돌을 회피.

```rust
// App (최종 형태)
struct App {
    engine: Engine,
    primary_window: Option<TastyWindow>,  // Phase 2에서 HashMap으로 확장
    shell_setup_mode: bool,
    shell_setup_path: String,
    shell_setup_gpu: Option<GpuState>,
    shell_setup_winit: Option<Arc<Window>>,
}

// App::window_event
fn window_event(&mut self, ..., event: WindowEvent) {
    if let Some(w) = &mut self.primary_window {
        w.handle_event(&mut self.engine, event_loop, event);
    }
}
```

### 단계

1. `TastyWindow`에 이벤트 핸들러 메서드 구현
   - `handle_event()`: egui 이벤트 전달 + match 분기
   - `handle_keyboard_input()`: 기존 App 메서드에서 이동
   - `handle_ime()`: 이동
   - `handle_cursor_moved()`, `handle_mouse_input()`, `handle_mouse_wheel()`: 이동
   - `handle_redraw()`: 렌더링 트리거
2. `shortcuts.rs`의 `handle_shortcut()`을 `TastyWindow`의 메서드로 이동
3. `App::window_event()`를 위임 구조로 변경
4. `App`에서 gpu, state, window, dirty, modifiers, window_focused, cursor_position, dragging_divider, clipboard, preedit_text 필드 제거

### IPC handler 문제

IPC handler가 `AppState`를 받는데, `AppState`는 TastyWindow 안에 있게 된다. IPC는 Engine에서 처리하므로, IPC 커맨드를 채널로 받아 TastyWindow에서 처리하는 현재 구조를 유지한다.

```rust
// Engine::try_recv_ipc() → IpcCommand
// TastyWindow에서 process_ipc(engine) 호출 시 engine.try_recv_ipc()로 커맨드를 가져와 self.state에서 처리
```

IPC handler 내부의 `state.active_workspace` 접근은 TastyWindow의 AppState를 통해 그대로 동작한다. 멀티 윈도우(Phase 2)에서 "어떤 윈도우의 active_workspace인가"는 Engine에 `focused_window_id`를 추가하여 해결.

---

## Phase 1-3: 헤드리스 모드 제거

헤드리스 모드(`--headless`, `run_headless`)를 제거한다. 터미널 로직 테스트는 `EngineState`를 직접 생성하여 수행.

### 삭제 대상
- `main.rs`의 `run_headless()` 함수
- `cli.rs`의 `--headless` CLI 플래그
- 헤드리스 관련 문서/테스트 참조

### 테스트 전환
- 기존 헤드리스 E2E 테스트 → `EngineState` + IPC 서버를 직접 조합하는 방식으로 전환
- 또는 GUI 테스트(`GuiTestInstance`)로 통합

---

## Phase 2: 멀티 윈도우 지원

### 2-1. App을 HashMap으로 확장

```rust
struct App {
    engine: Engine,
    windows: HashMap<WindowId, TastyWindow>,
    shell_setup_mode: bool,
    shell_setup_path: String,
    shell_setup_gpu: Option<GpuState>,
    shell_setup_winit: Option<Arc<Window>>,
}
```

### 2-2. 윈도우 생성/파괴

- `Engine::create_window()` → 새 OS 윈도우 + wgpu 서피스 + egui 초기화
- 마지막 윈도우 닫히면 앱 종료

### 2-3. wgpu 리소스 공유

adapter/device를 Engine으로 올림. 윈도우는 surface만 소유.

단일 adapter로 시작. 멀티 GPU 환경에서 surface 생성 실패 시 에러 로그 + 해당 윈도우 생성 거부.

### 2-4. IPC 확장

| 메서드 | 설명 |
|--------|------|
| `window.list` | 윈도우 목록 |
| `window.create` | 새 윈도우 생성 |
| `window.close` | 윈도우 닫기 |
| `window.focus` | 윈도우 포커스 |

Engine에 `focused_window_id: Option<WindowId>` 추가. 기존 IPC 메서드에서 `state.active_workspace`는 focused window의 AppState를 참조.

---

## Phase 3: 모달 시스템

### 3-1. 모달 윈도우 타입

`WindowKind` enum으로 관리:

```rust
enum WindowKind {
    Terminal(TastyWindow),
    Modal(ModalWindow),  // egui 전용, CellRenderer 없음
}
```

### 3-2. 포커스 차단

`Engine.modal_window_id: Option<WindowId>`를 사용. `windows` HashMap에 해당 ID가 없으면 자동 해제 (모달 크래시 시 자동 복구).

각 `TastyWindow::handle_event()`에서:

```rust
if engine.is_modal_active() {
    return; // 입력 무시
}
```

### 3-3. 설정창을 독립 모달로

설정 단축키 → Engine이 ModalWindow 생성, modal_window_id 설정.
모달 닫기 → modal_window_id = None, 설정 변경 사항을 Engine에 반영.

---

## 실행 순서

1. ~~Phase 0-1: EngineState 추출~~ ✅
2. ~~Phase 0-2: 영향 파일 수정~~ ✅
3. ~~Phase 0-3: render() 분해~~ ✅
4. ~~Phase 0-4: event_handler 분해~~ ✅
5. ~~Phase 1-1: Engine 구조체~~ ✅
6. **Phase 1-2: TastyWindow로 윈도우 필드 이동** ← 다음
7. **Phase 1-3: 헤드리스 모드 제거**
8. **Phase 2: 멀티 윈도우 (HashMap, 생성/파괴, wgpu 공유, IPC)**
9. **Phase 3: 모달 시스템 (WindowKind, 포커스 차단, 설정창 분리)**

**각 Phase는 독립적으로 빌드 + 실행 가능해야 한다.**

---

## 주의사항

### 관심사 분리

- 하나의 함수는 하나의 역할
- 하나의 구조체는 하나의 소유권 범위
- IPC는 Engine 경유, UI는 TastyWindow 경유

### borrow checker 대응

- `App`이 `engine`과 `windows`를 별도 필드로 소유 → 동시 `&mut` 가능
- `Arc<Mutex<>>` 불필요 (winit 이벤트 루프가 단일 스레드)
- TastyWindow의 메서드에 `engine: &mut Engine`을 파라미터로 전달

### 크로스플랫폼

- Engine의 wgpu adapter/device: 플랫폼 독립
- TastyWindow의 surface: winit이 처리
- 모달 포커스 차단: 앱 레벨 (OS API 미사용)
