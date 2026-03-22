# portable-pty 0.8 사용 가이드

portable-pty 0.8 기준. Windows(ConPTY), macOS, Linux(Unix PTY)를 동일한 API로 추상화한다.

---

## 목차

1. [PtySystem 얻기](#1-ptysystem-얻기)
2. [PtySize / PtyPair](#2-ptysize--ptypair)
3. [MasterPty](#3-masterpty)
4. [CommandBuilder](#4-commandbuilder)
5. [SlavePty와 Child](#5-slavePty와-child)
6. [Child](#6-child)
7. [Windows ConPTY vs Unix PTY 차이점](#7-windows-conpty-vs-unix-pty-차이점)
8. [전체 통합 예시](#8-전체-통합-예시)

---

## 1. PtySystem 얻기

`native_pty_system()`은 현재 플랫폼에 맞는 PTY 구현을 반환한다.

```rust
use portable_pty::{native_pty_system, PtySystem};

let pty_system = native_pty_system();
```

반환 타입은 `Box<dyn PtySystem>`. 플랫폼별 구현:
- Windows: `ConPtySystem` (Windows 10 1809 이상의 ConPTY API 사용)
- macOS/Linux: `UnixPtySystem` (openpty 기반)

### `PtySystem::openpty(size) -> Result<PtyPair>`

PTY 마스터/슬레이브 쌍을 생성한다.

```rust
let pair = pty_system.openpty(PtySize {
    rows: 24,
    cols: 80,
    pixel_width: 0,
    pixel_height: 0,
})?;
```

---

## 2. PtySize / PtyPair

### PtySize

```rust
use portable_pty::PtySize;

let size = PtySize {
    rows: 24,           // 터미널 높이 (행 수)
    cols: 80,           // 터미널 너비 (열 수)
    pixel_width: 0,     // 픽셀 너비 (0이면 무시)
    pixel_height: 0,    // 픽셀 높이 (0이면 무시)
};
```

`pixel_width` / `pixel_height`는 일부 터미널 앱이 폰트 크기 계산에 사용한다. 보통 0으로 설정해도 무방하다.

### PtyPair

`openpty()`가 반환하는 마스터/슬레이브 쌍.

```rust
use portable_pty::PtyPair;

let PtyPair { master, slave } = pty_system.openpty(size)?;
```

- `master: Box<dyn MasterPty>` — 터미널 에뮬레이터가 사용
- `slave: Box<dyn SlavePty>` — 자식 프로세스가 사용

---

## 3. MasterPty

`MasterPty`는 터미널 에뮬레이터 측의 PTY 인터페이스다.

```rust
use portable_pty::MasterPty;
```

### `resize(size) -> Result<()>`

실행 중인 프로세스에 터미널 크기 변경을 알린다. SIGWINCH를 보내는 것과 동일한 효과.

```rust
master.resize(PtySize {
    rows: new_rows,
    cols: new_cols,
    pixel_width: 0,
    pixel_height: 0,
})?;
```

창 크기 변경 이벤트(`winit::event::WindowEvent::Resized`) 처리 시 호출한다.

### `try_clone_reader() -> Result<Box<dyn Read + Send>>`

마스터에서 출력을 읽는 리더를 복제한다. 별도 스레드에서 PTY 출력을 읽을 때 사용한다.

```rust
let reader = master.try_clone_reader()?;
// reader: Box<dyn Read + Send>

std::thread::spawn(move || {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                // buf[..n]을 터미널 파서로 전달
            }
        }
    }
});
```

**주의:** 복제된 리더와 원본 마스터는 같은 파일 디스크립터를 공유한다. 여러 스레드에서 동시에 읽으면 데이터가 분산된다. 리더는 하나의 스레드에서만 사용해야 한다.

### `take_writer() -> Result<Box<dyn Write + Send>>`

마스터에 입력을 쓰는 라이터를 가져온다. 키 입력을 PTY로 전달할 때 사용한다.

```rust
let mut writer = master.take_writer()?;
// writer: Box<dyn Write + Send>

// 키 입력 전달
writer.write_all(b"ls\r")?;
writer.flush()?;
```

`take_writer()`는 한 번만 호출 가능하다. 두 번 호출하면 에러가 발생한다.

---

## 4. CommandBuilder

`CommandBuilder`는 자식 프로세스 실행 명령을 구성한다.

```rust
use portable_pty::CommandBuilder;
```

### `new(program)`

지정된 프로그램으로 커맨드 빌더를 생성한다.

```rust
let cmd = CommandBuilder::new("bash");
let cmd = CommandBuilder::new("cmd.exe");
let cmd = CommandBuilder::new("/usr/bin/zsh");
```

### `new_default_prog()`

현재 사용자의 기본 셸을 자동으로 선택한다.
- Windows: `cmd.exe` 또는 `COMSPEC` 환경 변수
- Unix: `$SHELL` 환경 변수, 없으면 `/bin/sh`

```rust
let cmd = CommandBuilder::new_default_prog();
```

### `arg(arg)`

인자를 추가한다. 체이닝 가능.

```rust
let cmd = CommandBuilder::new("ssh")
    .arg("-p")
    .arg("2222")
    .arg("user@host");
```

**주의:** `arg()`는 `CommandBuilder`를 소비하지 않는다. `&mut self`를 반환하므로 변수에 할당해야 한다.

```rust
let mut cmd = CommandBuilder::new("ssh");
cmd.arg("-p");
cmd.arg("2222");
cmd.arg("user@host");
```

### `env(key, value)`

환경 변수를 설정한다.

```rust
let mut cmd = CommandBuilder::new("bash");
cmd.env("TERM", "xterm-256color");
cmd.env("COLORTERM", "truecolor");
cmd.env("LANG", "ko_KR.UTF-8");
```

### `cwd(path)`

작업 디렉터리를 설정한다.

```rust
cmd.cwd("/home/user/project");
// 또는
cmd.cwd(std::env::current_dir()?);
```

### `get_argv() -> &[String]`

설정된 인자 목록을 확인한다.

```rust
let args = cmd.get_argv();
```

---

## 5. SlavePty와 Child

### SlavePty

슬레이브 PTY에서 자식 프로세스를 실행한다.

```rust
use portable_pty::SlavePty;
```

### `spawn_command(cmd) -> Result<Box<dyn Child + Send + Sync>>`

CommandBuilder로 자식 프로세스를 시작한다. 슬레이브 PTY가 자식 프로세스의 stdin/stdout/stderr가 된다.

```rust
let child = slave.spawn_command(cmd)?;
```

**중요:** 자식 프로세스를 시작한 뒤 슬레이브 PTY는 마스터 측에서 더 이상 필요하지 않다. `drop(slave)`를 호출해 파일 디스크립터를 닫아야 자식 프로세스 종료 시 마스터에서 EOF를 감지할 수 있다.

```rust
let child = slave.spawn_command(cmd)?;
drop(slave); // 반드시 닫을 것
```

---

## 6. Child

`Child`는 실행 중인 자식 프로세스를 제어하는 인터페이스다.

```rust
use portable_pty::Child;
```

### `process_id() -> Option<u32>`

자식 프로세스의 PID를 반환한다.

```rust
if let Some(pid) = child.process_id() {
    println!("PID: {}", pid);
}
```

### `try_wait() -> Result<Option<ExitStatus>>`

블로킹 없이 종료 상태를 확인한다.

```rust
match child.try_wait()? {
    None => {
        // 아직 실행 중
    }
    Some(status) => {
        println!("종료 코드: {:?}", status);
    }
}
```

### `wait() -> Result<ExitStatus>`

자식 프로세스가 종료될 때까지 블로킹한다.

```rust
let status = child.wait()?;
```

**주의:** `wait()`는 블로킹이므로 별도 스레드에서 호출하거나 비동기 런타임과 통합해야 한다.

### `kill() -> Result<()>`

자식 프로세스를 강제 종료한다.
- Unix: `SIGKILL`
- Windows: `TerminateProcess`

```rust
child.kill()?;
```

### `clone_killer() -> Box<dyn ChildKiller + Send + Sync>`

`kill()` 기능만 가진 핸들을 복제한다. 다른 스레드에서 프로세스를 종료할 때 유용하다.

```rust
let killer = child.clone_killer();

// 다른 스레드에서:
std::thread::spawn(move || {
    std::thread::sleep(std::time::Duration::from_secs(30));
    killer.kill().ok(); // 타임아웃 후 강제 종료
});
```

`ChildKiller` 트레이트:
```rust
pub trait ChildKiller {
    fn kill(&mut self) -> Result<()>;
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync>;
}
```

### ExitStatus

```rust
use portable_pty::ExitStatus;

match status {
    ExitStatus::Exited(code) => println!("정상 종료: {}", code),
    ExitStatus::Signal(sig) => println!("시그널 종료: {}", sig), // Unix만
}
```

`status.success()` — 종료 코드가 0이면 `true`.

---

## 7. Windows ConPTY vs Unix PTY 차이점

### 주요 차이점 요약

| 항목 | Windows ConPTY | Unix PTY |
|------|----------------|----------|
| 최소 요구 버전 | Windows 10 버전 1809+ | 모든 Unix |
| 구현 방식 | `CreatePseudoConsole` Win32 API | `openpty(3)` + fork/exec |
| 슬레이브 FD | 없음 (파이프로 대체) | 파일 디스크립터 |
| 시그널 | SIGWINCH 없음 (`ResizePseudoConsole` 사용) | SIGWINCH |
| 프로세스 그룹 | 없음 | 있음 (세션 리더) |
| ANSI 처리 | 호스트 측에서 처리 가능 | 커널 라인 디스플린 |
| PTY 복제 | `try_clone_reader`가 파이프 복제 | dup(2) |

### Windows에서의 주의사항

**ConPTY는 Windows 10 1809(빌드 17763) 이상에서만 동작한다.**

```rust
// Cargo.toml
[target.'cfg(windows)'.dependencies]
portable-pty = { version = "0.8", features = ["win32-input-mode"] }
```

`win32-input-mode` 피처를 활성화하면 Win32 입력 모드를 지원해 마우스 이벤트와 확장 키 입력이 가능해진다.

**환경 변수 TERM:**
ConPTY는 `xterm-256color`를 권장한다. `TERM=xterm-256color`를 명시적으로 설정하는 것이 좋다.

```rust
let mut cmd = CommandBuilder::new_default_prog();
cmd.env("TERM", "xterm-256color");
```

**슬레이브 drop 시점:**
Windows ConPTY에서는 슬레이브를 즉시 drop해도 되지만, Unix에서는 자식 프로세스가 시작된 직후에 drop해야 EOF가 제대로 전파된다. 공통 코드로 작성할 때는 `spawn_command()` 직후에 `drop(slave)`를 호출하면 양 플랫폼 모두에서 안전하다.

**리사이즈:**
Unix에서는 `resize()`가 내부적으로 `ioctl(TIOCSWINSZ)`를 호출하고 자식에게 `SIGWINCH`를 보낸다. Windows에서는 `ResizePseudoConsole()`을 호출한다. API는 동일하므로 플랫폼별 분기 없이 사용 가능하다.

**스레드 안전성:**
Windows ConPTY의 `MasterPty`는 내부적으로 두 파이프(읽기/쓰기)를 사용한다. `try_clone_reader()`는 읽기 파이프를, `take_writer()`는 쓰기 파이프를 각각 별도 스레드에서 안전하게 사용할 수 있다.

### Unix 전용 기능

Unix 전용으로 `portable_pty::unix` 모듈이 있지만, 크로스 플랫폼 코드에서는 사용하지 않는 것이 좋다.

```rust
// Unix 전용: 시그널 직접 전송
#[cfg(unix)]
{
    use std::os::unix::process::CommandExt;
    // nix 크레이트를 함께 사용
}
```

---

## 8. 전체 통합 예시

터미널 에뮬레이터에서 PTY를 생성하고 사용하는 완전한 예시.

```rust
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

pub struct PtySession {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtySession {
    pub fn new(cols: u16, rows: u16) -> anyhow::Result<(Self, Box<dyn Read + Send>)> {
        let pty_system = native_pty_system();

        // PTY 쌍 생성
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // 자식 프로세스 명령 구성
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        // 현재 디렉터리를 작업 디렉터리로 설정
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        // 자식 프로세스 시작
        let child = pair.slave.spawn_command(cmd)?;

        // 슬레이브를 즉시 해제해야 자식 종료 시 EOF가 전파됨
        drop(pair.slave);

        // 리더는 별도 스레드로 넘길 것이므로 분리
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let session = Self {
            master: pair.master,
            writer,
            child,
        };

        Ok((session, reader))
    }

    /// 키 입력을 PTY로 전달한다.
    pub fn write_input(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    /// 터미널 크기를 변경한다.
    pub fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// 자식 프로세스가 종료됐는지 확인한다.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// 자식 프로세스를 강제 종료한다.
    pub fn kill(&mut self) {
        self.child.kill().ok();
    }

    /// 자식 프로세스 PID를 반환한다.
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }
}

/// PTY 출력을 읽는 스레드를 시작한다.
/// 수신한 데이터는 콜백으로 전달된다.
pub fn start_pty_reader<F>(
    mut reader: Box<dyn Read + Send>,
    mut on_data: F,
) -> std::thread::JoinHandle<()>
where
    F: FnMut(&[u8]) + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    // EOF — 자식 프로세스가 슬레이브 PTY를 닫음
                    break;
                }
                Ok(n) => {
                    on_data(&buf[..n]);
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        std::thread::yield_now();
                        continue;
                    }
                    // 기타 에러 — 연결 종료로 간주
                    break;
                }
            }
        }
    })
}

/// 사용 예시
pub fn example() -> anyhow::Result<()> {
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    let (mut session, reader) = PtySession::new(80, 24)?;

    // PTY 출력 읽기 스레드
    let reader_thread = start_pty_reader(reader, move |data| {
        tx.send(data.to_vec()).ok();
    });

    // 터미널 파서 (termwiz)
    let mut parser = termwiz::escape::parser::Parser::new();

    // 메인 이벤트 루프
    loop {
        // PTY 출력 처리
        while let Ok(data) = rx.try_recv() {
            parser.parse(&data, |action| {
                // action 처리
                let _ = action;
            });
        }

        // 자식 프로세스 종료 확인
        if !session.is_alive() {
            break;
        }

        // 예시: 1초 후 리사이즈
        // session.resize(120, 40)?;

        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    reader_thread.join().ok();
    Ok(())
}
```

### 비동기 런타임(tokio)과의 통합

portable-pty는 동기 API만 제공하므로, tokio와 통합하려면 `spawn_blocking`을 사용한다.

```rust
use tokio::sync::mpsc;

pub async fn start_async_pty(
    cols: u16,
    rows: u16,
) -> anyhow::Result<(PtySession, mpsc::Receiver<Vec<u8>>)> {
    let (session, reader) = PtySession::new(cols, rows)?;
    let (tx, rx) = mpsc::channel(256);

    // 블로킹 읽기를 별도 스레드에서 실행
    tokio::task::spawn_blocking(move || {
        start_pty_reader(reader, move |data| {
            tx.blocking_send(data.to_vec()).ok();
        })
        .join()
        .ok();
    });

    Ok((session, rx))
}
```

### 키 입력 변환 예시

winit 키보드 이벤트를 터미널 입력 바이트로 변환한다.

```rust
use winit::keyboard::{Key, NamedKey};
use winit::event::ElementState;

pub fn key_to_pty_bytes(
    key: &Key,
    modifiers: winit::event::Modifiers,
) -> Option<Vec<u8>> {
    let ctrl = modifiers.state().control_key();
    let shift = modifiers.state().shift_key();

    match key {
        Key::Named(NamedKey::Enter) => Some(b"\r".to_vec()),
        Key::Named(NamedKey::Backspace) => Some(b"\x7f".to_vec()),
        Key::Named(NamedKey::Escape) => Some(b"\x1b".to_vec()),
        Key::Named(NamedKey::Tab) => {
            if shift { Some(b"\x1b[Z".to_vec()) } // Shift+Tab
            else { Some(b"\t".to_vec()) }
        }
        Key::Named(NamedKey::ArrowUp) => Some(b"\x1b[A".to_vec()),
        Key::Named(NamedKey::ArrowDown) => Some(b"\x1b[B".to_vec()),
        Key::Named(NamedKey::ArrowRight) => Some(b"\x1b[C".to_vec()),
        Key::Named(NamedKey::ArrowLeft) => Some(b"\x1b[D".to_vec()),
        Key::Named(NamedKey::Home) => Some(b"\x1b[H".to_vec()),
        Key::Named(NamedKey::End) => Some(b"\x1b[F".to_vec()),
        Key::Named(NamedKey::PageUp) => Some(b"\x1b[5~".to_vec()),
        Key::Named(NamedKey::PageDown) => Some(b"\x1b[6~".to_vec()),
        Key::Named(NamedKey::Delete) => Some(b"\x1b[3~".to_vec()),
        Key::Named(NamedKey::Insert) => Some(b"\x1b[2~".to_vec()),
        Key::Character(s) if ctrl => {
            // Ctrl+A~Z → \x01~\x1a
            let c = s.chars().next()?;
            let b = c.to_ascii_lowercase() as u8;
            if (b'a'..=b'z').contains(&b) {
                Some(vec![b - b'a' + 1])
            } else {
                None
            }
        }
        Key::Character(s) => {
            Some(s.as_bytes().to_vec())
        }
        _ => None,
    }
}
```
