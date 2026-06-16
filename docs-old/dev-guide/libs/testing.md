# testing

터미널 에뮬레이터의 자동화 테스트를 위한 라이브러리. `enigo`는 키보드/마우스 입력 시뮬레이션, `windows`는 Windows API 직접 호출을 담당한다.

## Cargo.toml

```toml
[dependencies]
enigo = "0.6"

[target.'cfg(windows)'.dependencies]
windows = { version = "0.62", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Gdi",
] }
```

## enigo 0.6 — 입력 시뮬레이션

### Enigo 생성

```rust
use enigo::{Enigo, Settings};

// 기본 설정으로 생성
let mut enigo = Enigo::new(&Settings::default()).unwrap();
```

### Key 열거형

```rust
use enigo::Key;

// 일반 문자
Key::Unicode('a')
Key::Unicode('A')
Key::Unicode('가')

// 특수 키
Key::Return      // Enter
Key::Tab
Key::Space
Key::Backspace
Key::Delete
Key::Escape

// 방향키
Key::UpArrow
Key::DownArrow
Key::LeftArrow
Key::RightArrow

// 기능키
Key::F1 .. Key::F12

// 수정자 키
Key::Shift
Key::Control
Key::Alt
Key::Meta  // Windows 키 / Command 키 (macOS)
Key::CapsLock

// 특수
Key::Home
Key::End
Key::PageUp
Key::PageDown
```

### Direction 열거형

```rust
use enigo::Direction;

Direction::Press    // 키 누름
Direction::Release  // 키 뗌
Direction::Click    // 누름 + 뗌 (Press + Release)
```

### Keyboard 트레이트

```rust
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

let mut enigo = Enigo::new(&Settings::default()).unwrap();

// 문자 타이핑
enigo.key(Key::Unicode('a'), Direction::Click).unwrap();

// 텍스트 입력 (한 번에)
enigo.text("Hello, 터미널!\n").unwrap();

// 키 누름/뗌 분리
enigo.key(Key::Shift, Direction::Press).unwrap();
enigo.key(Key::Unicode('a'), Direction::Click).unwrap();  // 'A'
enigo.key(Key::Shift, Direction::Release).unwrap();
```

### Mouse 트레이트

```rust
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};

let mut enigo = Enigo::new(&Settings::default()).unwrap();

// 마우스 이동 (절대 좌표)
enigo.move_mouse(100, 200, Coordinate::Abs).unwrap();

// 마우스 이동 (상대 좌표)
enigo.move_mouse(10, -5, Coordinate::Rel).unwrap();

// 클릭
enigo.button(Button::Left, Direction::Click).unwrap();
enigo.button(Button::Right, Direction::Click).unwrap();

// 더블클릭
enigo.button(Button::Left, Direction::Click).unwrap();
enigo.button(Button::Left, Direction::Click).unwrap();

// 스크롤 (세로)
enigo.scroll(3, Axis::Vertical).unwrap();   // 아래로 3
enigo.scroll(-3, Axis::Vertical).unwrap();  // 위로 3
```

### 조합키 시뮬레이션

```rust
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::time::Duration;

fn send_key_combo(enigo: &mut Enigo, modifier: Key, key: Key) {
    enigo.key(modifier, Direction::Press).unwrap();
    std::thread::sleep(Duration::from_millis(50));  // 안정성을 위한 짧은 대기
    enigo.key(key, Direction::Click).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    enigo.key(modifier, Direction::Release).unwrap();
}

fn main() {
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    // Ctrl+C (복사)
    send_key_combo(&mut enigo, Key::Control, Key::Unicode('c'));

    // Ctrl+V (붙여넣기)
    send_key_combo(&mut enigo, Key::Control, Key::Unicode('v'));

    // Alt+F4 (창 닫기)
    send_key_combo(&mut enigo, Key::Alt, Key::F4);

    // Ctrl+Shift+T (새 탭)
    enigo.key(Key::Control, Direction::Press).unwrap();
    enigo.key(Key::Shift, Direction::Press).unwrap();
    enigo.key(Key::Unicode('t'), Direction::Click).unwrap();
    enigo.key(Key::Shift, Direction::Release).unwrap();
    enigo.key(Key::Control, Direction::Release).unwrap();
}
```

### 터미널 자동화 테스트 패턴

```rust
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::time::Duration;

pub struct TerminalTester {
    enigo: Enigo,
    delay: Duration,
}

impl TerminalTester {
    pub fn new() -> Self {
        Self {
            enigo: Enigo::new(&Settings::default()).unwrap(),
            delay: Duration::from_millis(100),
        }
    }

    /// 터미널에 텍스트 입력
    pub fn type_text(&mut self, text: &str) {
        self.enigo.text(text).unwrap();
        std::thread::sleep(self.delay);
    }

    /// 커맨드 실행 (텍스트 + Enter)
    pub fn run_command(&mut self, cmd: &str) {
        self.type_text(cmd);
        self.enigo.key(Key::Return, Direction::Click).unwrap();
        std::thread::sleep(Duration::from_millis(500));  // 커맨드 완료 대기
    }

    /// Ctrl+C (실행 중단)
    pub fn interrupt(&mut self) {
        self.enigo.key(Key::Control, Direction::Press).unwrap();
        self.enigo.key(Key::Unicode('c'), Direction::Click).unwrap();
        self.enigo.key(Key::Control, Direction::Release).unwrap();
        std::thread::sleep(self.delay);
    }

    /// 창 포커스 후 커맨드 실행
    pub fn focus_and_run(&mut self, hwnd: isize, cmd: &str) {
        #[cfg(windows)]
        focus_window(hwnd);
        std::thread::sleep(Duration::from_millis(200));
        self.run_command(cmd);
    }
}
```

### 플랫폼별 주의사항 (enigo)

| 플랫폼 | 주의 |
|--------|------|
| Windows | UAC로 보호된 창에는 입력 전달 불가. 관리자 권한 필요할 수 있음. |
| macOS | `Accessibility` 권한 필요. 시스템 환경설정 → 개인 정보 보호 → 손쉬운 사용 |
| Linux (X11) | `DISPLAY` 환경변수 필요. `libxdo` 또는 `xtest` 필요. |
| Linux (Wayland) | 제한적 지원. 일부 기능 미작동 가능. |

## windows 0.62 — Windows API

### 필요한 피처

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.62", features = [
    "Win32_Foundation",              # HWND, BOOL, RECT 등 기본 타입
    "Win32_UI_WindowsAndMessaging",  # FindWindowW, SetForegroundWindow 등
    "Win32_Graphics_Gdi",            # GetClientRect, 화면 관련
] }
```

### HWND 타입

```rust
#[cfg(windows)]
use windows::Win32::Foundation::HWND;

// HWND는 *mut c_void 래퍼 (null 가능)
// 안전한 사용: isize로 전달하고 필요할 때만 HWND로 변환
fn to_hwnd(handle: isize) -> HWND {
    HWND(handle as *mut _)
}
```

### FindWindowW — 창 찾기

```rust
#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::UI::WindowsAndMessaging::FindWindowW,
};

#[cfg(windows)]
fn find_window_by_title(title: &str) -> Option<isize> {
    // UTF-16 변환
    let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let hwnd = FindWindowW(PCWSTR::null(), PCWSTR(title_wide.as_ptr()));
        if hwnd.0.is_null() {
            None
        } else {
            Some(hwnd.0 as isize)
        }
    }
}

#[cfg(windows)]
fn find_window_by_class(class_name: &str) -> Option<isize> {
    let class_wide: Vec<u16> = class_name.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let hwnd = FindWindowW(PCWSTR(class_wide.as_ptr()), PCWSTR::null());
        if hwnd.0.is_null() {
            None
        } else {
            Some(hwnd.0 as isize)
        }
    }
}
```

### SetForegroundWindow — 창 포커스

```rust
#[cfg(windows)]
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::SetForegroundWindow,
};

#[cfg(windows)]
fn focus_window(hwnd_handle: isize) -> bool {
    let hwnd = HWND(hwnd_handle as *mut _);
    unsafe { SetForegroundWindow(hwnd).as_bool() }
}

// 주의: Windows Vista+ 에서 SetForegroundWindow는 제한적으로 작동.
// 현재 포그라운드 프로세스와 동일한 프로세스이거나,
// 사용자 입력을 최근에 받은 경우에만 성공.
```

### ShowWindow — 창 상태 변경

```rust
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    ShowWindow,
    SW_MAXIMIZE, SW_MINIMIZE, SW_NORMAL, SW_RESTORE, SW_HIDE, SW_SHOW,
    SHOW_WINDOW_CMD,
};

#[cfg(windows)]
fn maximize_window(hwnd_handle: isize) {
    let hwnd = HWND(hwnd_handle as *mut _);
    unsafe { ShowWindow(hwnd, SW_MAXIMIZE); }
}

#[cfg(windows)]
fn minimize_window(hwnd_handle: isize) {
    let hwnd = HWND(hwnd_handle as *mut _);
    unsafe { ShowWindow(hwnd, SW_MINIMIZE); }
}

#[cfg(windows)]
fn restore_window(hwnd_handle: isize) {
    let hwnd = HWND(hwnd_handle as *mut _);
    unsafe { ShowWindow(hwnd, SW_RESTORE); }
}
```

### GetClientRect / GetWindowRect — 창 크기

```rust
#[cfg(windows)]
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
};

#[derive(Debug, Clone, Copy)]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WindowRect {
    pub fn width(&self) -> i32 { self.right - self.left }
    pub fn height(&self) -> i32 { self.bottom - self.top }
}

/// 클라이언트 영역 크기 (타이틀바, 테두리 제외)
#[cfg(windows)]
fn get_client_rect(hwnd_handle: isize) -> Option<WindowRect> {
    let hwnd = HWND(hwnd_handle as *mut _);
    let mut rect = RECT::default();
    unsafe {
        if GetClientRect(hwnd, &mut rect).is_ok() {
            Some(WindowRect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
        } else {
            None
        }
    }
}

/// 전체 창 크기 (타이틀바, 테두리 포함)
#[cfg(windows)]
fn get_window_rect(hwnd_handle: isize) -> Option<WindowRect> {
    let hwnd = HWND(hwnd_handle as *mut _);
    let mut rect = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut rect).is_ok() {
            Some(WindowRect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
        } else {
            None
        }
    }
}
```

### 통합 자동화 테스트 예시

```rust
#[cfg(test)]
#[cfg(windows)]
mod integration_tests {
    use super::*;
    use enigo::{Enigo, Settings};
    use std::time::Duration;

    fn wait(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    #[test]
    #[ignore]  // 수동 실행 테스트 (CI에서 제외)
    fn test_terminal_basic_input() {
        // 1. Tasty 터미널 창 찾기
        let hwnd = find_window_by_title("Tasty Terminal")
            .expect("터미널 창을 찾을 수 없음");

        // 2. 창 포커스
        focus_window(hwnd);
        wait(300);

        // 3. 클라이언트 영역 확인
        let rect = get_client_rect(hwnd).unwrap();
        println!("창 크기: {}x{}", rect.width(), rect.height());
        assert!(rect.width() > 100);
        assert!(rect.height() > 100);

        // 4. 입력 시뮬레이션
        let mut enigo = Enigo::new(&Settings::default()).unwrap();
        let mut tester = TerminalTester::with_enigo(enigo);

        tester.run_command("echo hello");
        wait(500);

        // 5. 최소화/복원
        minimize_window(hwnd);
        wait(300);
        restore_window(hwnd);
        wait(300);
    }
}
```

### 안전성 주의사항

| 항목 | 주의 |
|------|------|
| `unsafe` 블록 | Win32 API는 모두 `unsafe`. 반환값으로 성공 여부 확인 필수. |
| HWND 유효성 | 창이 닫힌 후 HWND는 무효. 사용 전 `IsWindow()` 확인 권장. |
| 스레드 안전 | Win32 창 API는 창을 생성한 스레드에서 호출해야 함. |
| 문자열 | 모든 Win32 문자열 함수는 UTF-16 (`PCWSTR`). 변환 필수. |
