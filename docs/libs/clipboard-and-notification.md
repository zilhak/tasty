# clipboard-and-notification

터미널 에뮬레이터에서 클립보드 접근과 데스크탑 알림을 처리한다.

## Cargo.toml

```toml
[dependencies]
arboard = "3"
notify-rust = "4"
```

## arboard 3 — 클립보드

### Clipboard 생성

```rust
use arboard::Clipboard;

fn main() -> Result<(), arboard::Error> {
    // 시스템 클립보드 핸들 생성
    let mut clipboard = Clipboard::new()?;

    // 텍스트 쓰기
    clipboard.set_text("복사된 내용")?;

    // 텍스트 읽기
    let text = clipboard.get_text()?;
    println!("클립보드 내용: {text}");

    Ok(())
}
```

### get_text

```rust
use arboard::Clipboard;

fn read_clipboard() -> Result<String, arboard::Error> {
    let mut clipboard = Clipboard::new()?;
    let text = clipboard.get_text()?;
    Ok(text)
}

// 빈 클립보드 처리
fn read_clipboard_safe() -> String {
    Clipboard::new()
        .and_then(|mut c| c.get_text())
        .unwrap_or_default()
}
```

### set_text

```rust
use arboard::Clipboard;

fn write_clipboard(text: &str) -> Result<(), arboard::Error> {
    let mut clipboard = Clipboard::new()?;
    clipboard.set_text(text)?;
    Ok(())
}

// 긴 텍스트 (터미널 출력 복사)
fn copy_terminal_output(output: &str) -> Result<(), arboard::Error> {
    let mut clipboard = Clipboard::new()?;
    // 앞뒤 공백 정리 후 복사
    clipboard.set_text(output.trim())?;
    Ok(())
}
```

### 이미지 클립보드 (스크린샷 등)

```rust
use arboard::{Clipboard, ImageData};
use std::borrow::Cow;

fn copy_image(rgba_data: Vec<u8>, width: usize, height: usize) -> Result<(), arboard::Error> {
    let mut clipboard = Clipboard::new()?;
    let image = ImageData {
        width,
        height,
        bytes: Cow::Owned(rgba_data),
    };
    clipboard.set_image(image)?;
    Ok(())
}

fn read_image() -> Result<Option<(Vec<u8>, usize, usize)>, arboard::Error> {
    let mut clipboard = Clipboard::new()?;
    match clipboard.get_image() {
        Ok(img) => Ok(Some((img.bytes.into_owned(), img.width, img.height))),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(e) => Err(e),
    }
}
```

### 에러 처리

```rust
use arboard::{Clipboard, Error as ClipboardError};

fn handle_clipboard_error(e: &ClipboardError) {
    match e {
        ClipboardError::ContentNotAvailable => {
            // 클립보드가 비어있거나 지원하지 않는 형식
            eprintln!("클립보드 내용 없음");
        }
        ClipboardError::ClipboardNotSupported => {
            // 현재 환경에서 클립보드 미지원 (예: 헤드리스 서버)
            eprintln!("클립보드 미지원 환경");
        }
        ClipboardError::ClipboardOccupied => {
            // 다른 프로세스가 클립보드 사용 중
            eprintln!("클립보드 사용 중");
        }
        ClipboardError::ConversionFailure => {
            eprintln!("데이터 변환 실패");
        }
        ClipboardError::Unknown { description } => {
            eprintln!("알 수 없는 오류: {description}");
        }
        _ => eprintln!("클립보드 오류: {e}"),
    }
}

// 실용적인 래퍼
pub struct ClipboardManager;

impl ClipboardManager {
    pub fn copy(text: &str) -> bool {
        match Clipboard::new().and_then(|mut c| c.set_text(text)) {
            Ok(()) => true,
            Err(e) => {
                handle_clipboard_error(&e);
                false
            }
        }
    }

    pub fn paste() -> Option<String> {
        match Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(text) => Some(text),
            Err(ClipboardError::ContentNotAvailable) => None,
            Err(e) => {
                handle_clipboard_error(&e);
                None
            }
        }
    }
}
```

### 플랫폼별 주의사항 (arboard)

| 플랫폼 | 주의 |
|--------|------|
| Linux (X11) | `Clipboard` 객체가 살아있는 동안만 데이터 유지. 프로세스 종료 시 소멸. `xclip`/`xsel` 없어도 작동. |
| Linux (Wayland) | `wl-clipboard` 또는 `wl-paste` 필요. `WAYLAND_DISPLAY` 환경변수 확인. |
| Windows | WinAPI 직접 사용. `set_text` 호출 후 즉시 드롭해도 데이터 유지. |
| macOS | Pasteboard API 사용. `NSString` UTF-16 변환 자동 처리. |

```rust
// Linux Wayland에서 실패하면 X11 폴백
fn clipboard_with_fallback() -> Option<Clipboard> {
    Clipboard::new().ok()
    // arboard 3는 자동으로 Wayland → X11 폴백을 시도함
}
```

## notify-rust 4 — 데스크탑 알림

### 기본 알림

```rust
use notify_rust::Notification;

fn send_notification() -> Result<(), notify_rust::error::Error> {
    Notification::new()
        .summary("Tasty 터미널")
        .body("빌드가 완료되었습니다.")
        .show()?;
    Ok(())
}
```

### summary / body / show

```rust
use notify_rust::Notification;

Notification::new()
    .summary("작업 완료")                    // 제목 (굵게 표시)
    .body("cargo build 성공 (3.2s)")         // 본문
    .icon("terminal")                        // 아이콘 이름 (freedesktop 표준)
    .timeout(notify_rust::Timeout::Milliseconds(5000))  // 5초 후 자동 닫기
    .show()
    .unwrap();

// 영구 알림 (사용자가 닫을 때까지)
Notification::new()
    .summary("오류 발생")
    .body("PTY 연결이 끊겼습니다.")
    .urgency(notify_rust::Urgency::Critical)
    .timeout(notify_rust::Timeout::Never)
    .show()
    .unwrap();
```

### urgency 레벨

```rust
use notify_rust::{Notification, Urgency};

// 낮은 중요도 (조용히 표시)
Notification::new()
    .summary("알림")
    .urgency(Urgency::Low)
    .show().ok();

// 보통 (기본값)
Notification::new()
    .summary("알림")
    .urgency(Urgency::Normal)
    .show().ok();

// 긴급 (빨간색 또는 강조 표시, 자동 닫기 무시)
Notification::new()
    .summary("긴급")
    .urgency(Urgency::Critical)
    .show().ok();
```

### 알림 핸들 및 업데이트 (Linux D-Bus)

```rust
use notify_rust::Notification;

// Linux에서는 알림 핸들을 통해 업데이트/닫기 가능
let handle = Notification::new()
    .summary("진행 중...")
    .body("0%")
    .show()
    .unwrap();

// 알림 업데이트
for i in 1..=10 {
    std::thread::sleep(std::time::Duration::from_millis(200));
    handle.summary(&format!("진행 중... {}", i * 10));
    handle.update();
}

// 알림 닫기
handle.close();
```

### 터미널 에뮬레이터 알림 패턴

```rust
use notify_rust::{Notification, Timeout, Urgency};

pub struct TerminalNotifier {
    app_name: String,
    app_icon: String,
}

impl TerminalNotifier {
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            app_icon: "utilities-terminal".to_string(),
        }
    }

    /// 명령 완료 알림 (포커스를 잃었을 때만 유용)
    pub fn command_finished(&self, command: &str, exit_code: i32, duration_secs: f64) {
        let (summary, urgency) = if exit_code == 0 {
            (format!("{} 완료", self.app_name), Urgency::Normal)
        } else {
            (format!("{} 실패 (exit {})", self.app_name, exit_code), Urgency::Critical)
        };

        let body = format!("`{}` — {:.1}초", command, duration_secs);

        Notification::new()
            .summary(&summary)
            .body(&body)
            .icon(&self.app_icon)
            .urgency(urgency)
            .timeout(Timeout::Milliseconds(4000))
            .show()
            .ok();  // 알림 실패는 무시 (비필수 기능)
    }

    /// 연결 끊김 알림
    pub fn session_disconnected(&self, session_id: &str) {
        Notification::new()
            .summary(&format!("{} 세션 종료", self.app_name))
            .body(&format!("세션 {} 이 종료되었습니다.", session_id))
            .icon(&self.app_icon)
            .urgency(Urgency::Normal)
            .timeout(Timeout::Milliseconds(3000))
            .show()
            .ok();
    }
}
```

### 플랫폼별 주의사항 (notify-rust)

| 플랫폼 | 구현 | 주의 |
|--------|------|------|
| Linux | D-Bus (`org.freedesktop.Notifications`) | `libdbus-1-dev` 필요. 데스크탑 환경 필요. |
| macOS | `NSUserNotificationCenter` / `UNUserNotificationCenter` | macOS 10.14+ 에서 `UNUserNotification` 사용. 권한 요청 필요. |
| Windows | Windows Toast Notification | `windows` 피처 활성화 필요. `AppUserModelId` 설정 권장. |

```toml
# Windows 지원 활성화
[target.'cfg(windows)'.dependencies]
notify-rust = { version = "4", features = ["d", "z"] }

# 또는 전체 플랫폼
notify-rust = { version = "4" }
```

```rust
// 플랫폼별 처리
#[cfg(target_os = "linux")]
fn notify(summary: &str, body: &str) {
    Notification::new()
        .summary(summary)
        .body(body)
        .hint(notify_rust::Hint::Category("x-gnome-terminal".to_string()))
        .show()
        .ok();
}

#[cfg(target_os = "macos")]
fn notify(summary: &str, body: &str) {
    Notification::new()
        .summary(summary)
        .body(body)
        .show()
        .ok();
}

#[cfg(target_os = "windows")]
fn notify(summary: &str, body: &str) {
    Notification::new()
        .summary(summary)
        .body(body)
        .show()
        .ok();
}
```

### 알림 지원 여부 확인

```rust
/// 현재 환경에서 알림이 가능한지 확인
pub fn notifications_available() -> bool {
    Notification::new()
        .summary("test")
        .show()
        .is_ok()
}

/// 알림을 안전하게 전송 (실패해도 패닉 없음)
pub fn try_notify(summary: &str, body: &str) {
    if let Err(e) = Notification::new().summary(summary).body(body).show() {
        // 알림 실패는 로그만 남기고 계속 진행
        tracing::debug!("알림 전송 실패: {e}");
    }
}
```
