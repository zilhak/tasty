# winit 0.30 사용 가이드

winit은 크로스 플랫폼 창 생성 및 이벤트 처리 라이브러리입니다. 0.30 버전에서는 `ApplicationHandler` 트레이트 기반의 새로운 API로 전면 개편되었습니다.

---

## 목차

1. [ApplicationHandler 트레이트](#applicationhandler-트레이트)
2. [WindowEvent 변형](#windowevent-변형)
3. [KeyEvent 구조체 상세](#keyevent-구조체-상세)
4. [터미널 에뮬레이터 키 입력 처리 패턴](#터미널-에뮬레이터-키-입력-처리-패턴)
5. [Key, NamedKey, ModifiersState](#key-namedkey-modifiersstate)
6. [EventLoop과 EventLoopProxy](#eventloop과-eventloopproxy)
7. [Window와 WindowAttributes](#window와-windowattributes)
8. [DPI 좌표계](#dpi-좌표계)
9. [ControlFlow](#controlflow)

---

## ApplicationHandler 트레이트

winit 0.30의 핵심 변경점입니다. 이전 버전의 `EventLoop::run` 클로저 방식 대신, `ApplicationHandler` 트레이트를 구현한 구조체를 전달하는 방식으로 변경되었습니다.

```rust
use winit::application::ApplicationHandler;
use winit::event::{WindowEvent, DeviceEvent, DeviceId};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    /// 앱이 실행 준비가 되었을 때 호출됩니다.
    /// 창 생성은 반드시 여기서 해야 합니다 (new()에서 하면 안 됨).
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attrs = Window::default_attributes()
            .with_title("My App")
            .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0));

        self.window = Some(event_loop.create_window(window_attrs).unwrap());
    }

    /// 창별 이벤트를 처리합니다.
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // 렌더링 로직
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    /// UserEvent를 처리합니다 (EventLoopProxy를 통해 전송된 커스텀 이벤트).
    /// EventLoop<T>에서 T가 UserEvent 타입입니다.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        // 커스텀 이벤트 처리
    }

    /// 이벤트 큐가 비어서 대기 상태로 전환되기 직전에 호출됩니다.
    /// 이 시점에 렌더링이나 상태 업데이트를 수행하는 것이 권장됩니다.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App { window: None };
    event_loop.run_app(&mut app).unwrap();
}
```

### 메서드 호출 순서

```
EventLoop 시작
    → resumed() — 창 생성
    → window_event() — 각종 이벤트 처리
    → about_to_wait() — 이벤트 큐 비었을 때
    → (반복)
    → window_event(CloseRequested) — 종료 요청
    → EventLoop 종료
```

### suspended (모바일/웹 전용)

`suspended()`는 iOS/Android에서 앱이 백그라운드로 내려갈 때 호출됩니다. 데스크톱에서는 호출되지 않으므로 일반적으로 구현하지 않아도 됩니다.

---

## WindowEvent 변형

`window_event()` 메서드에서 처리하는 `WindowEvent` 열거형의 주요 변형들입니다.

### KeyboardInput

```rust
WindowEvent::KeyboardInput {
    device_id,
    event,          // KeyEvent 구조체 (아래 상세 설명)
    is_synthetic,   // IME 합성 이벤트 여부
} => {
    // 키 입력 처리
}
```

### MouseInput

```rust
WindowEvent::MouseInput {
    device_id,
    state,          // ElementState::Pressed | ElementState::Released
    button,         // MouseButton::Left | Right | Middle | Back | Forward | Other(u16)
} => {
    match (button, state) {
        (MouseButton::Left, ElementState::Pressed) => { /* 클릭 */ }
        _ => {}
    }
}
```

### CursorMoved

```rust
WindowEvent::CursorMoved {
    device_id,
    position,   // PhysicalPosition<f64> — 물리 픽셀 단위
} => {
    let (x, y) = (position.x, position.y);
}
```

### MouseWheel

```rust
WindowEvent::MouseWheel {
    device_id,
    delta,      // MouseScrollDelta
    phase,      // TouchPhase
} => {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => {
            // 라인 단위 스크롤 (마우스 휠)
            // y > 0: 위로, y < 0: 아래로
        }
        MouseScrollDelta::PixelDelta(pos) => {
            // 픽셀 단위 스크롤 (트랙패드)
        }
    }
}
```

### Resized

```rust
WindowEvent::Resized(new_size) => {
    // new_size: PhysicalSize<u32>
    // GPU Surface 재구성 필요
    let width = new_size.width;
    let height = new_size.height;
    // surface.configure(...) 재호출
}
```

### ScaleFactorChanged

```rust
WindowEvent::ScaleFactorChanged {
    scale_factor,       // f64 — 새 DPI 스케일 (예: 1.0, 1.5, 2.0)
    inner_size_writer,  // 새 물리 크기를 직접 설정할 수 있는 핸들
} => {
    // 폰트 크기, 레이아웃 등 DPI 의존적인 값 재계산
    // inner_size_writer.request_inner_size(new_size) 로 창 크기 강제 설정 가능
}
```

### Focused

```rust
WindowEvent::Focused(focused) => {
    if focused {
        // 창이 포커스를 얻음 — 커서 표시 재시작 등
    } else {
        // 창이 포커스를 잃음 — 키 상태 초기화 권장
    }
}
```

### Occluded

```rust
WindowEvent::Occluded(occluded) => {
    if occluded {
        // 창이 다른 창에 가려짐 — 렌더링 일시 중단 가능
    } else {
        // 창이 다시 보임
    }
}
```

### RedrawRequested

```rust
WindowEvent::RedrawRequested => {
    // 실제 렌더링 수행
    // window.request_redraw()로 다음 프레임 요청 가능
    render_frame();
}
```

### CloseRequested

```rust
WindowEvent::CloseRequested => {
    // X 버튼 클릭 등 닫기 요청
    // 실제로 닫으려면 event_loop.exit() 호출
    event_loop.exit();
}
```

### 전체 변형 요약표

| 변형 | 트리거 | 주요 필드 |
|------|--------|-----------|
| `KeyboardInput` | 키 누름/뗌 | `event: KeyEvent` |
| `MouseInput` | 마우스 버튼 | `button`, `state` |
| `CursorMoved` | 마우스 이동 | `position: PhysicalPosition` |
| `MouseWheel` | 휠 스크롤 | `delta: MouseScrollDelta` |
| `Resized` | 창 크기 변경 | `PhysicalSize<u32>` |
| `ScaleFactorChanged` | DPI 변경 | `scale_factor: f64` |
| `Focused` | 포커스 변경 | `bool` |
| `Occluded` | 가림 상태 변경 | `bool` |
| `RedrawRequested` | 재그리기 요청 | - |
| `CloseRequested` | 닫기 요청 | - |
| `CursorEntered` | 커서 진입 | `device_id` |
| `CursorLeft` | 커서 이탈 | `device_id` |
| `Ime` | IME 입력 | `Ime` 열거형 |
| `Touch` | 터치 이벤트 | `Touch` 구조체 |
| `DroppedFile` | 파일 드롭 | `PathBuf` |
| `HoveredFile` | 파일 호버 | `PathBuf` |
| `ThemeChanged` | 테마 변경 | `Theme` |
| `Destroyed` | 창 소멸 | - |

---

## KeyEvent 구조체 상세

터미널 에뮬레이터 구현에서 가장 중요한 부분입니다. `KeyEvent` 구조체를 올바르게 이해하지 않으면 키 입력 버그가 발생합니다.

```rust
pub struct KeyEvent {
    pub physical_key: PhysicalKey,   // 키보드의 물리적 위치 (레이아웃 무관)
    pub logical_key: Key,            // 현재 레이아웃 기준 논리적 키
    pub text: Option<SmolStr>,       // 실제로 입력된 텍스트 (중요: 아래 설명 참고)
    pub location: KeyLocation,       // 키 위치 (Standard, Left, Right, Numpad)
    pub state: ElementState,         // Pressed | Released
    pub repeat: bool,                // 키 반복 입력 여부
    // ... 내부 필드
}
```

### physical_key vs logical_key vs text 차이

| 필드 | 타입 | 설명 | 예시 (Shift+A) |
|------|------|------|----------------|
| `physical_key` | `PhysicalKey` | 키보드 스캔코드 기반 위치 | `KeyCode::KeyA` |
| `logical_key` | `Key` | 현재 언어/레이아웃 적용된 키 | `Key::Character("A")` |
| `text` | `Option<SmolStr>` | 실제 입력 텍스트 | `Some("A")` |

#### physical_key (PhysicalKey)

레이아웃에 무관한 물리적 키 위치입니다. QWERTY 기준의 `KeyCode`로 표현됩니다.

```rust
use winit::keyboard::PhysicalKey;
use winit::keyboard::KeyCode;

match event.physical_key {
    PhysicalKey::Code(KeyCode::KeyA) => { /* 항상 'A' 키 위치 */ }
    PhysicalKey::Code(KeyCode::Escape) => { /* ESC 키 */ }
    PhysicalKey::Unidentified(_) => { /* 인식 불가 키 */ }
}
```

단축키나 게임 컨트롤에서 유용합니다. 언어 변경 시에도 동일한 위치를 가리킵니다.

#### logical_key (Key)

현재 키보드 레이아웃과 수정자(Shift, AltGr 등)가 적용된 논리적 키입니다.

```rust
use winit::keyboard::{Key, NamedKey};

match &event.logical_key {
    Key::Character(ch) => { /* 일반 문자 키 */ }
    Key::Named(NamedKey::Enter) => { /* Enter 키 */ }
    Key::Named(NamedKey::Backspace) => { /* Backspace 키 */ }
    Key::Named(NamedKey::Tab) => { /* Tab 키 */ }
    Key::Named(NamedKey::Escape) => { /* ESC 키 */ }
    _ => {}
}
```

#### text 필드 — 버그의 원인

`text` 필드는 **IME 처리가 완료된 후 실제로 화면에 입력될 문자열**입니다.

**중요한 주의사항:**

```
특수키(Backspace, Enter, Tab, Escape, 방향키 등)의 text 값:
- 일부 플랫폼에서 Some("\x08")  ← Backspace 제어 문자 (\x08 = BS)
- 일부 플랫폼에서 Some("\r")    ← Enter 제어 문자
- 일부 플랫폼에서 Some("\t")    ← Tab 문자
- 일부 플랫폼에서 None          ← 없는 경우도 있음
```

**버그 시나리오:** `text`만 사용해서 키 입력을 처리하면, Backspace를 눌렀을 때 `\x08`이 텍스트로 입력되거나, Enter를 눌렀을 때 제어 문자가 버퍼에 들어가는 문제가 발생합니다.

```rust
// 잘못된 방식 — text만 사용
if let Some(text) = &event.text {
    terminal.write(text.as_str()); // Backspace가 "\x08"로 입력될 수 있음!
}
```

---

## 터미널 에뮬레이터 키 입력 처리 패턴

올바른 패턴은 **특수키를 먼저 처리하고, `text`는 fallback으로만 사용**하는 것입니다.

```rust
use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{Key, NamedKey, ModifiersState};

fn handle_key_event(
    event: &winit::event::KeyEvent,
    modifiers: ModifiersState,
    pty_writer: &mut dyn std::io::Write,
) {
    // Pressed 이벤트만 처리 (또는 repeat도 포함)
    if event.state == ElementState::Released {
        return;
    }

    // 1단계: logical_key의 NamedKey로 특수키 먼저 처리
    if let Key::Named(named) = &event.logical_key {
        let sequence: Option<&[u8]> = match named {
            NamedKey::Enter => Some(b"\r"),
            NamedKey::Backspace => Some(b"\x7f"), // 터미널에서 DEL이 표준
            NamedKey::Tab => {
                if modifiers.shift_key() {
                    Some(b"\x1b[Z")  // Shift+Tab = Reverse Tab
                } else {
                    Some(b"\t")
                }
            }
            NamedKey::Escape => Some(b"\x1b"),
            NamedKey::ArrowUp => Some(b"\x1b[A"),
            NamedKey::ArrowDown => Some(b"\x1b[B"),
            NamedKey::ArrowRight => Some(b"\x1b[C"),
            NamedKey::ArrowLeft => Some(b"\x1b[D"),
            NamedKey::Home => Some(b"\x1b[H"),
            NamedKey::End => Some(b"\x1b[F"),
            NamedKey::PageUp => Some(b"\x1b[5~"),
            NamedKey::PageDown => Some(b"\x1b[6~"),
            NamedKey::Insert => Some(b"\x1b[2~"),
            NamedKey::Delete => Some(b"\x1b[3~"),
            NamedKey::F1 => Some(b"\x1bOP"),
            NamedKey::F2 => Some(b"\x1bOQ"),
            NamedKey::F3 => Some(b"\x1bOR"),
            NamedKey::F4 => Some(b"\x1bOS"),
            NamedKey::F5 => Some(b"\x1b[15~"),
            NamedKey::F6 => Some(b"\x1b[17~"),
            NamedKey::F7 => Some(b"\x1b[18~"),
            NamedKey::F8 => Some(b"\x1b[19~"),
            NamedKey::F9 => Some(b"\x1b[20~"),
            NamedKey::F10 => Some(b"\x1b[21~"),
            NamedKey::F11 => Some(b"\x1b[23~"),
            NamedKey::F12 => Some(b"\x1b[24~"),
            _ => None,
        };

        if let Some(seq) = sequence {
            pty_writer.write_all(seq).ok();
            return; // 특수키 처리 완료, text로 넘어가지 않음
        }
    }

    // 2단계: Ctrl 수정자 처리
    if modifiers.control_key() {
        if let Key::Character(ch) = &event.logical_key {
            if let Some(c) = ch.chars().next() {
                let ctrl_char = match c.to_ascii_lowercase() {
                    'a'..='z' => Some((c as u8 - b'a' + 1) as char),
                    '[' => Some('\x1b'), // Ctrl+[ = ESC
                    '\\' => Some('\x1c'),
                    ']' => Some('\x1d'),
                    '^' => Some('\x1e'),
                    '_' => Some('\x1f'),
                    _ => None,
                };
                if let Some(cc) = ctrl_char {
                    pty_writer.write_all(&[cc as u8]).ok();
                    return;
                }
            }
        }
    }

    // 3단계: text 필드를 fallback으로 사용 (일반 문자 입력)
    // 이 시점에서 text는 특수키가 아닌 실제 문자여야 함
    if let Some(text) = &event.text {
        // 제어 문자 필터링 — text에 제어 문자가 포함된 경우 무시
        let filtered: String = text.chars()
            .filter(|&c| !c.is_control() || c == '\t')
            .collect();
        if !filtered.is_empty() {
            pty_writer.write_all(filtered.as_bytes()).ok();
        }
    }
}
```

### 핵심 원칙 요약

1. **`Key::Named(NamedKey::*)`를 먼저 매칭** — Backspace, Enter, Tab 등을 명시적으로 처리
2. **각 특수키에 올바른 터미널 이스케이프 시퀀스 전송** — 단순히 `\x08`이 아닌 `\x7f` 등 터미널 표준 준수
3. **`text` 필드는 마지막 fallback으로만** — 특수키가 걸러진 후 남은 일반 문자에만 사용
4. **제어 문자 필터링** — text에서 제어 문자(`\x00`~`\x1f`, `\x7f`)를 제거

---

## Key, NamedKey, ModifiersState

### Key 열거형

```rust
pub enum Key {
    /// 문자 키 (예: 'a', 'A', '1', '!')
    Character(SmolStr),
    /// 이름 있는 특수키
    Named(NamedKey),
    /// 인식 불가 키
    Unidentified(NativeKey),
    /// 데드 키 (악센트 입력용, 예: ´, `)
    Dead(Option<char>),
}
```

### NamedKey 주요 변형

```rust
use winit::keyboard::NamedKey;

// 편집 키
NamedKey::Backspace
NamedKey::Delete
NamedKey::Insert
NamedKey::Enter
NamedKey::Tab
NamedKey::Escape

// 방향 키
NamedKey::ArrowUp
NamedKey::ArrowDown
NamedKey::ArrowLeft
NamedKey::ArrowRight

// 페이지 탐색
NamedKey::Home
NamedKey::End
NamedKey::PageUp
NamedKey::PageDown

// 기능 키
NamedKey::F1 ~ NamedKey::F35

// 수정자 키
NamedKey::Shift
NamedKey::Control
NamedKey::Alt
NamedKey::Super  // Windows/Cmd 키
NamedKey::CapsLock
NamedKey::NumLock

// 기타
NamedKey::Space
NamedKey::Copy
NamedKey::Cut
NamedKey::Paste
```

### ModifiersState

현재 수정자 키 상태를 추적합니다. `WindowEvent::ModifiersChanged`로 업데이트합니다.

```rust
use winit::keyboard::ModifiersState;

struct App {
    modifiers: ModifiersState,
    // ...
}

impl ApplicationHandler for App {
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // self.modifiers로 현재 수정자 상태 확인
                if self.modifiers.control_key() && /* ... */ {}
                if self.modifiers.shift_key() && /* ... */ {}
                if self.modifiers.alt_key() && /* ... */ {}
                if self.modifiers.super_key() && /* ... */ {}
            }
            _ => {}
        }
    }
    // ...
}
```

| 메서드 | 설명 |
|--------|------|
| `control_key()` | Ctrl 키가 눌려있는지 |
| `shift_key()` | Shift 키가 눌려있는지 |
| `alt_key()` | Alt/Option 키가 눌려있는지 |
| `super_key()` | Windows/Cmd 키가 눌려있는지 |

---

## EventLoop과 EventLoopProxy

### EventLoop 생성

```rust
use winit::event_loop::{EventLoop, ControlFlow};

// 기본 EventLoop
let event_loop = EventLoop::new().unwrap();

// 커스텀 이벤트 타입을 가진 EventLoop
#[derive(Debug)]
enum AppEvent {
    NewData(Vec<u8>),
    Shutdown,
}

let event_loop: EventLoop<AppEvent> = EventLoop::with_user_event().build().unwrap();
```

### EventLoopProxy — 백그라운드 스레드에서 이벤트 전송

`EventLoopProxy`를 사용하면 별도 스레드(PTY 읽기 스레드, 네트워크 스레드 등)에서 이벤트 루프를 깨울 수 있습니다.

```rust
use std::sync::Arc;
use std::thread;
use winit::event_loop::EventLoopProxy;

let proxy: EventLoopProxy<AppEvent> = event_loop.create_proxy();

// PTY 출력 읽기 스레드
thread::spawn(move || {
    let mut pty_reader = /* ... */;
    let mut buf = [0u8; 4096];

    loop {
        match pty_reader.read(&mut buf) {
            Ok(n) if n > 0 => {
                let data = buf[..n].to_vec();
                // 이벤트 루프로 데이터 전송 (스레드 안전)
                if proxy.send_event(AppEvent::NewData(data)).is_err() {
                    break; // EventLoop가 종료됨
                }
            }
            _ => break,
        }
    }
});

// ApplicationHandler::user_event()에서 처리
impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::NewData(data) => {
                self.terminal.process(&data);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            AppEvent::Shutdown => {
                event_loop.exit();
            }
        }
    }
    // ...
}
```

`send_event()`는 스레드 안전하며, `EventLoop`가 종료된 경우 `Err`를 반환합니다.

---

## Window와 WindowAttributes

### Window 생성

창은 반드시 `resumed()` 안에서 `event_loop.create_window()`로 생성해야 합니다.

```rust
use winit::window::{Window, WindowAttributes, WindowLevel, CursorIcon};
use winit::dpi::{LogicalSize, PhysicalSize};

fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let attrs = Window::default_attributes()
        .with_title("Tasty Terminal")
        .with_inner_size(LogicalSize::new(1200.0_f64, 800.0_f64))
        .with_min_inner_size(LogicalSize::new(200.0_f64, 100.0_f64))
        .with_resizable(true)
        .with_decorations(true)  // false: 타이틀바 없음
        .with_transparent(false)
        .with_window_level(WindowLevel::Normal);

    let window = event_loop.create_window(attrs).unwrap();
    self.window = Some(window);
}
```

### CursorIcon 변경

```rust
use winit::window::CursorIcon;

window.set_cursor(CursorIcon::Text);      // 텍스트 입력 커서 (I빔)
window.set_cursor(CursorIcon::Default);   // 기본 화살표
window.set_cursor(CursorIcon::Crosshair); // 십자선
window.set_cursor(CursorIcon::Hand);      // 링크 커서
window.set_cursor(CursorIcon::Wait);      // 로딩
window.set_cursor(CursorIcon::NotAllowed);// 금지
```

### 주요 Window 메서드

```rust
// 재그리기 요청
window.request_redraw();

// 창 크기
let physical_size: PhysicalSize<u32> = window.inner_size();
let logical_size: LogicalSize<f64> = window.inner_size().to_logical(window.scale_factor());

// DPI 스케일
let scale: f64 = window.scale_factor();

// 제목 변경
window.set_title("New Title");

// 전체화면
window.set_fullscreen(Some(Fullscreen::Borderless(None)));
window.set_fullscreen(None); // 해제

// 창 위치
window.set_outer_position(PhysicalPosition::new(100, 100));

// 커서 숨기기
window.set_cursor_visible(false);

// 포커스
window.focus_window();
```

### WindowAttributes 주요 옵션

| 메서드 | 설명 | 기본값 |
|--------|------|--------|
| `with_title` | 창 제목 | `""` |
| `with_inner_size` | 내부 크기 | OS 기본값 |
| `with_min_inner_size` | 최소 크기 | 제한 없음 |
| `with_max_inner_size` | 최대 크기 | 제한 없음 |
| `with_resizable` | 크기 조절 가능 | `true` |
| `with_decorations` | 창 테두리/타이틀바 | `true` |
| `with_transparent` | 투명 창 | `false` |
| `with_visible` | 창 표시 여부 | `true` |
| `with_window_icon` | 창 아이콘 | `None` |
| `with_window_level` | 창 레이어 순서 | `Normal` |

---

## DPI 좌표계

winit은 두 가지 좌표계를 사용합니다.

### Physical 좌표 (PhysicalSize, PhysicalPosition)

실제 화면 픽셀 단위입니다. GPU 렌더링과 `CursorMoved` 이벤트에서 사용됩니다.

```rust
use winit::dpi::{PhysicalSize, PhysicalPosition};

let size: PhysicalSize<u32> = window.inner_size();
// 4K 모니터 + 200% 스케일 → 3840x2160
```

### Logical 좌표 (LogicalSize, LogicalPosition)

DPI 스케일로 나눈 논리적 크기입니다. UI 레이아웃에 사용합니다.

```rust
use winit::dpi::{LogicalSize, LogicalPosition};

let scale = window.scale_factor(); // 예: 2.0
let logical: LogicalSize<f64> = window.inner_size().to_logical(scale);
// 3840x2160 물리 픽셀 + scale 2.0 → 1920x1080 논리 픽셀
```

### 변환

```rust
let scale_factor = window.scale_factor();

// Physical → Logical
let logical = physical_size.to_logical::<f64>(scale_factor);

// Logical → Physical
let physical = logical_size.to_physical::<u32>(scale_factor);
```

### 터미널에서의 활용

```rust
// 셀 크기 계산 (논리 픽셀 기반)
let cell_width_logical: f64 = 8.0;  // 폰트 크기에 따라
let cell_height_logical: f64 = 16.0;

let scale = window.scale_factor();
let window_size = window.inner_size(); // Physical

// 열/행 수 계산
let cols = (window_size.width as f64 / scale / cell_width_logical) as u16;
let rows = (window_size.height as f64 / scale / cell_height_logical) as u16;
```

---

## ControlFlow

이벤트 루프의 동작 방식을 제어합니다. `event_loop.set_control_flow()`로 설정합니다.

```rust
use winit::event_loop::ControlFlow;

// Wait: 다음 이벤트가 올 때까지 CPU 사용 없이 대기 (기본값, 절전 모드)
event_loop.set_control_flow(ControlFlow::Wait);

// Poll: 이벤트 없어도 계속 루프 실행 (게임, 실시간 렌더링)
event_loop.set_control_flow(ControlFlow::Poll);

// WaitUntil: 특정 시간까지 대기 (커서 깜빡임, 타이머 이벤트)
use std::time::{Duration, Instant};
event_loop.set_control_flow(ControlFlow::WaitUntil(
    Instant::now() + Duration::from_millis(16) // ~60fps
));
```

### 터미널 에뮬레이터 권장 설정

```rust
fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    if self.has_pending_output {
        // PTY에서 읽을 데이터가 있으면 즉시 처리
        event_loop.set_control_flow(ControlFlow::Poll);
        self.process_pty_output();
    } else {
        // 유휴 상태면 커서 깜빡임을 위해 타이머 설정
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(500)
        ));
    }
}
```

### EventLoop 종료

```rust
// window_event 또는 user_event 안에서
event_loop.exit();

// 다음 루프 사이클에서 종료됨 (현재 이벤트 처리 완료 후)
```

---

## 전체 예제

```rust
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

#[derive(Debug)]
enum AppEvent {
    PtyOutput(Vec<u8>),
}

struct TermApp {
    window: Option<Window>,
    modifiers: ModifiersState,
}

impl ApplicationHandler<AppEvent> for TermApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("Tasty Terminal")
            .with_inner_size(LogicalSize::new(1200.0_f64, 800.0_f64));
        let window = event_loop.create_window(attrs).unwrap();
        window.set_cursor(CursorIcon::Text);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed || event.repeat {
                    self.handle_key(&event);
                }
            }
            WindowEvent::Resized(new_size) => {
                // GPU surface 재구성
                println!("Resized: {}x{}", new_size.width, new_size.height);
            }
            WindowEvent::RedrawRequested => {
                // GPU 렌더링
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::PtyOutput(data) => {
                // 터미널 파서에 데이터 공급
                println!("PTY output: {} bytes", data.len());
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

impl TermApp {
    fn handle_key(&mut self, event: &winit::event::KeyEvent) {
        // 특수키 먼저 처리
        if let Key::Named(named) = &event.logical_key {
            let seq: Option<&[u8]> = match named {
                NamedKey::Enter => Some(b"\r"),
                NamedKey::Backspace => Some(b"\x7f"),
                NamedKey::Tab => Some(b"\t"),
                NamedKey::Escape => Some(b"\x1b"),
                NamedKey::ArrowUp => Some(b"\x1b[A"),
                NamedKey::ArrowDown => Some(b"\x1b[B"),
                NamedKey::ArrowRight => Some(b"\x1b[C"),
                NamedKey::ArrowLeft => Some(b"\x1b[D"),
                _ => None,
            };
            if let Some(s) = seq {
                // pty.write_all(s).ok();
                println!("Special key: {:?}", s);
                return;
            }
        }

        // 일반 문자는 text 필드 사용
        if let Some(text) = &event.text {
            let filtered: String = text.chars()
                .filter(|&c| !c.is_control())
                .collect();
            if !filtered.is_empty() {
                // pty.write_all(filtered.as_bytes()).ok();
                println!("Text input: {}", filtered);
            }
        }
    }
}

fn main() {
    let event_loop: EventLoop<AppEvent> = EventLoop::with_user_event().build().unwrap();
    let proxy = event_loop.create_proxy();

    // PTY 읽기 스레드 시작
    std::thread::spawn(move || {
        // 예시: 1초마다 데이터 전송
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if proxy.send_event(AppEvent::PtyOutput(b"hello\r\n".to_vec())).is_err() {
                break;
            }
        }
    });

    let mut app = TermApp {
        window: None,
        modifiers: ModifiersState::empty(),
    };
    event_loop.run_app(&mut app).unwrap();
}
```
