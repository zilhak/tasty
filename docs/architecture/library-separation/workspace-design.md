# Cargo Workspace 구조 설계

권장하는 2개 크레이트(`tasty-hooks`, `tasty-terminal`) 분리 후의 workspace 구조를 설계한다.

---

## 권장 디렉토리 구조

```
tasty/
├── Cargo.toml                  ← workspace 루트 + 바이너리 크레이트
├── Cargo.lock
├── CLAUDE.md
├── crates/
│   ├── tasty-hooks/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs          ← hooks.rs 내용 이동
│   └── tasty-terminal/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs          ← terminal.rs 내용 이동
├── src/
│   ├── main.rs                 ← mod hooks, mod terminal 제거
│   ├── cli.rs
│   ├── font.rs
│   ├── gpu.rs
│   ├── model.rs                ← use tasty_terminal::{Terminal, Waker};
│   ├── notification.rs
│   ├── renderer.rs
│   ├── settings.rs
│   ├── settings_ui.rs
│   ├── state.rs                ← use tasty_terminal::{Terminal, TerminalEvent, Waker};
│   ├── ui.rs
│   └── ipc/
│       ├── mod.rs
│       ├── handler.rs
│       ├── protocol.rs
│       └── server.rs
├── docs/
│   └── ...
└── target/
```

---

## Workspace Cargo.toml (루트)

```toml
[workspace]
members = [
    ".",
    "crates/tasty-hooks",
    "crates/tasty-terminal",
]
resolver = "3"

[package]
name = "tasty"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "Cross-platform GPU-accelerated terminal emulator for AI coding agents"
repository = "https://github.com/zilhak/tasty"

[dependencies]
# ---- workspace 내부 크레이트 ----
tasty-hooks = { path = "crates/tasty-hooks" }
tasty-terminal = { path = "crates/tasty-terminal" }

# ---- Window & Input ----
winit = "0.30"

# ---- GPU Rendering ----
wgpu = "24"

# ---- UI Widgets ----
egui = "0.31"
egui-wgpu = "0.31"
egui-winit = "0.31"

# ---- Terminal Emulation (tasty-terminal에서도 사용, 공유) ----
termwiz = "0.22"

# ---- Async ----
pollster = "0.4"
tokio = { version = "1", features = ["full"] }

# ---- Logging ----
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# ---- Error Handling ----
anyhow = "1"
thiserror = "2"

# ---- Serialization ----
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# ---- CLI ----
clap = { version = "4", features = ["derive"] }

# ---- Font & Text ----
cosmic-text = "0.18"
bytemuck = { version = "1", features = ["derive"] }

# ---- Notifications ----
notify-rust = "4"

# ---- Shell escaping ----
shell-escape = "0.1"

# ---- Clipboard ----
arboard = "3"

# ---- Utilities ----
directories = "6"

[profile.release]
lto = true
strip = true
opt-level = 3
```

주의: `portable-pty`와 `regex`는 루트에서 제거. 각각 `tasty-terminal`과 `tasty-hooks`의 내부 의존으로 이동.

---

## tasty-hooks Cargo.toml

```toml
[package]
name = "tasty-hooks"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "Event-driven hook system for terminal automation"
repository = "https://github.com/zilhak/tasty"
readme = "README.md"
keywords = ["terminal", "hooks", "automation", "ai-agent"]
categories = ["command-line-utilities", "development-tools"]

[dependencies]
regex = "1"
serde = { version = "1", features = ["derive"] }
```

### 의존성 설명

| 의존 | 용도 | 필수 |
|------|------|------|
| `regex` | OutputMatch 이벤트의 정규식 매칭 (`hooks.rs:44`) | 필수 |
| `serde` | HookEvent의 Serialize/Deserialize (`hooks.rs:17`) | 필수 |

---

## tasty-terminal Cargo.toml

```toml
[package]
name = "tasty-terminal"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "PTY-backed terminal emulation engine with Read Mark API"
repository = "https://github.com/zilhak/tasty"
readme = "README.md"
keywords = ["terminal", "pty", "vte", "ai-agent"]
categories = ["command-line-utilities", "development-tools"]

[dependencies]
# PTY
portable-pty = "0.8"

# VTE parsing
termwiz = "0.22"

# Regex (ANSI strip in read_since_mark)
regex = "1"

# Error handling
anyhow = "1"

# Logging
tracing = "0.1"
```

### 의존성 설명

| 의존 | 용도 | 필수 |
|------|------|------|
| `portable-pty` | PTY 생성 및 관리 (`terminal.rs:6`) | 필수 |
| `termwiz` | VTE 파서, Surface, Cell 타입 (`terminal.rs:7-16`) | 필수 |
| `regex` | read_since_mark의 ANSI 이스케이프 제거 | 필수 |
| `anyhow` | 에러 타입 (`terminal.rs:5`) | 필수 |
| `tracing` | 로그 매크로 | 선택적이지만 유지 권장 |

---

## 의존성 그래프

```
                 ┌──────────────────────────┐
                 │    tasty (바이너리)        │
                 │                          │
                 │  winit, wgpu, egui,      │
                 │  cosmic-text, termwiz,   │
                 │  serde_json, clap, ...   │
                 └───────┬──────────┬───────┘
                         │          │
                    path │          │ path
                         │          │
              ┌──────────▼──┐  ┌───▼──────────┐
              │ tasty-hooks │  │tasty-terminal │
              │             │  │              │
              │  regex      │  │ portable-pty │
              │  serde      │  │ termwiz      │
              │             │  │ regex        │
              └─────────────┘  │ anyhow       │
                               │ tracing      │
                               └──────────────┘

  참고: tasty-hooks와 tasty-terminal은 서로 의존하지 않음
```

### 의존 방향 규칙

```
금지: tasty-hooks → tasty-terminal  (순환 의존 방지)
금지: tasty-terminal → tasty-hooks  (순환 의존 방지)
허용: tasty (바이너리) → tasty-hooks
허용: tasty (바이너리) → tasty-terminal
```

---

## 공유 의존성 관리

### workspace.dependencies (Cargo 1.64+)

공유 의존성 버전을 workspace 루트에서 관리:

```toml
# Cargo.toml (루트)
[workspace.dependencies]
regex = "1"
serde = { version = "1", features = ["derive"] }
anyhow = "1"
tracing = "0.1"
termwiz = "0.22"

# crates/tasty-hooks/Cargo.toml
[dependencies]
regex = { workspace = true }
serde = { workspace = true }

# crates/tasty-terminal/Cargo.toml
[dependencies]
termwiz = { workspace = true }
regex = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
```

장점:
- 모든 크레이트가 동일 버전의 의존을 사용
- 버전 업그레이드가 한 곳에서 완결
- Cargo.lock이 중복 의존 방지

---

## Feature Flags 설계

### tasty-hooks

```toml
[features]
default = ["serde"]
serde = ["dep:serde"]
```

`serde` feature를 선택적으로 만들면, 직렬화가 필요 없는 사용자는 serde 의존을 제거할 수 있다. 단, 현재 `HookEvent`의 `serde(rename_all)` 속성이 IPC에서 사용되므로 기본 활성화.

### tasty-terminal

```toml
[features]
default = ["read-mark"]
read-mark = ["regex"]
```

Read Mark API의 ANSI 제거 기능이 `regex`에 의존하므로, `regex` 없이 사용하고 싶은 경우를 위해 feature flag 제공. 단, 현재 regex는 가벼운 의존이므로 기본 활성화 권장.

### 바이너리 (tasty)

```toml
[features]
default = ["system-notification"]
system-notification = ["notify-rust"]
```

`notify-rust`를 feature flag으로 만들면 headless 환경에서 OS 알림 없이 빌드 가능. 이것은 크레이트 분리와 무관하지만, workspace 전환 시 함께 적용하면 좋음.

---

## 빌드 명령어 예시

```bash
# 전체 빌드
cargo build

# 전체 테스트
cargo test

# hooks만 테스트
cargo test -p tasty-hooks

# terminal만 테스트
cargo test -p tasty-terminal

# hooks만 린트
cargo clippy -p tasty-hooks

# 전체 문서 생성
cargo doc --workspace --no-deps

# 릴리스 빌드
cargo build --release

# 특정 크레이트의 의존 트리 확인
cargo tree -p tasty-hooks

# workspace 전체 의존 트리
cargo tree --workspace
```
