# terminal.rs 분할 계획

`src/terminal.rs` (1,358줄)를 `src/terminal/` 디렉토리로 분할한다.

## 현재 구조 분석

terminal.rs는 하나의 `Terminal` struct에 대한 거대한 `impl` 블록과 이벤트 타입, 테스트로 구성된다.

| 줄 범위 | 내용 |
|---------|------|
| 1-16 | use 선언 (std, termwiz, portable-pty) |
| 18-19 | `Waker` type alias |
| 21-42 | `TerminalEvent`, `TerminalEventKind` |
| 44-45 | `OUTPUT_BUFFER_MAX` 상수 |
| 47-54 | `MouseTrackingMode` enum |
| 56-97 | `Terminal` struct (필드 정의) |
| 99-179 | `impl Terminal`: `new`, `new_with_shell` (생성자) |
| 181-231 | `impl Terminal`: `process` (메인 루프) |
| 233-247 | `impl Terminal`: `action_to_changes` (VTE 액션 디스패치) |
| 249-269 | `impl Terminal`: `map_control` |
| 271-286 | `impl Terminal`: `map_csi` |
| 288-319 | `impl Terminal`: `map_sgr` |
| 321-421 | `impl Terminal`: `map_cursor` |
| 423-650 | `impl Terminal`: `map_edit` |
| 652-696 | `impl Terminal`: `map_esc` |
| 698-794 | `impl Terminal`: `map_osc` |
| 796-806 | `impl Terminal`: `send_key`, `send_bytes` |
| 808-825 | `impl Terminal`: `resize` |
| 827-845 | `impl Terminal`: `surface`, `surface_mut` |
| 847-893 | `impl Terminal`: getters (cols, rows, is_alive, take_events, set_mark, read_since_mark) |
| 892-1019 | `impl Terminal`: `handle_mode`, `set_dec_mode` (DECSET/DECRST) |
| 1021-1093 | `impl Terminal`: `scroll_region_params`, `read_line_from_surface`, getters, `default_shell` |
| 1096-1104 | `ANSI_ESCAPE_RE`, `strip_ansi_escapes` 유틸리티 |
| 1106-1358 | `#[cfg(test)] mod tests` |

## 분할 후 구조

```
src/terminal/
├── mod.rs              — Terminal struct, 핵심 API (new, process, send, resize, getters)
├── vte_handler.rs      — VTE 액션 → Surface 변환 (action_to_changes, map_* 함수들)
├── modes.rs            — DECSET/DECRST 모드 핸들링 (handle_mode, set_dec_mode)
├── events.rs           — TerminalEvent, TerminalEventKind, Waker, MouseTrackingMode
└── tests.rs            — 모든 테스트
```

## 각 파일 상세

### events.rs (~40줄)

독립 타입 정의. Terminal struct에 대한 의존 없음.

**포함 내용:**
- `Waker` type alias (줄 19)
- `TerminalEvent` struct (줄 22-26)
- `TerminalEventKind` enum (줄 29-42)
- `OUTPUT_BUFFER_MAX` 상수 (줄 45)
- `MouseTrackingMode` enum (줄 48-54)

**의존:** 없음 (std::sync::Arc만 사용)

### mod.rs (~350줄)

Terminal struct 정의와 핵심 public API를 담는다.

**포함 내용:**
- `Terminal` struct 필드 정의 (줄 56-97)
- `new`, `new_with_shell` (줄 99-179)
- `process` (줄 181-231)
- `send_key`, `send_bytes` (줄 796-806)
- `resize` (줄 808-825)
- `surface`, `surface_mut` (줄 827-845)
- getters: `cols`, `rows`, `is_alive`, `check_process_alive`, `take_events`, `set_mark`, `read_since_mark` (줄 847-893)
- `scroll_region_params` (줄 1021-1033)
- `read_line_from_surface` (줄 1035-1052)
- public getters: `application_cursor_keys`, `cursor_visible`, `bracketed_paste`, `mouse_tracking`, `sgr_mouse`, `focus_tracking`, `is_alternate_screen` (줄 1054-1089)
- `default_shell` (줄 1091-1093)
- `ANSI_ESCAPE_RE`, `strip_ansi_escapes` (줄 1096-1104)

**의존:**
- `events::{Waker, TerminalEvent, TerminalEventKind, OUTPUT_BUFFER_MAX, MouseTrackingMode}`
- 외부: portable-pty, termwiz, std

### vte_handler.rs (~430줄)

VTE 파싱 결과를 termwiz Surface의 Change로 변환하는 모든 `map_*` 함수.

**포함 내용:**
- `action_to_changes` (줄 233-247)
- `map_control` (줄 249-269)
- `map_csi` (줄 271-286)
- `map_sgr` (줄 288-319)
- `map_cursor` (줄 321-421)
- `map_edit` (줄 423-650)
- `map_esc` (줄 652-696)
- `map_osc` (줄 698-794)

**의존:**
- `super::Terminal` (self 참조)
- `super::events::{TerminalEvent, TerminalEventKind}`
- termwiz 타입

### modes.rs (~130줄)

DECSET/DECRST 모드 토글 로직.

**포함 내용:**
- `handle_mode` (줄 892-906)
- `set_dec_mode` (줄 908-1019)

**의존:**
- `super::Terminal` (self 참조)
- `super::events::MouseTrackingMode`
- termwiz: `CsiMode`, `DecPrivateMode`, `DecPrivateModeCode`

### tests.rs (~250줄)

**포함 내용:**
- `#[cfg(test)] mod tests` 전체 (줄 1106-1358)
- DECSET/DECRST 토글 테스트 (줄 1118-1227)
- 대체 화면 테스트 (줄 1229-1304)
- 화살표 키 모드 테스트 (줄 1306-1325)
- 전체 리셋 테스트 (줄 1327-1358)

## impl 블록 분산 패턴

Rust에서 하나의 struct에 대해 여러 파일에서 `impl` 블록을 작성할 수 있다. `Terminal`은 `mod.rs`에 정의하고, `vte_handler.rs`와 `modes.rs`에서 추가 `impl Terminal` 블록을 작성한다.

```rust
// vte_handler.rs
use super::Terminal;
use super::events::{TerminalEvent, TerminalEventKind};

impl Terminal {
    /// Convert a parsed VT action into Surface changes.
    pub(super) fn action_to_changes(&mut self, action: Action) -> Vec<Change> {
        // ...
    }

    fn map_control(&mut self, code: ControlCode) -> Vec<Change> {
        // ...
    }
    // ... 나머지 map_* 함수들
}
```

```rust
// modes.rs
use super::Terminal;
use super::events::MouseTrackingMode;

impl Terminal {
    pub(super) fn handle_mode(&mut self, mode: &CsiMode) {
        // ...
    }

    fn set_dec_mode(&mut self, code: &DecPrivateModeCode, enable: bool) {
        // ...
    }
}
```

이 패턴의 장점:
1. Terminal의 내부 필드 접근을 유지하면서 파일을 분리할 수 있다.
2. `map_*` 함수들이 `self.surface()`, `self.surface_mut()`, `self.events` 등 Terminal 내부에 직접 접근하므로, trait이나 헬퍼 구조체로 분리하면 불필요한 복잡성이 생긴다.
3. 가시성은 `pub(super)`로 제한하여 모듈 외부 노출을 방지한다.

## tasty-terminal crate 추출과의 연계

이 분할은 `tasty-terminal` crate 추출의 사전 작업이다.

| 분할 파일 | crate 추출 시 |
|----------|-------------|
| `events.rs` | `tasty-terminal/src/events.rs` → public API |
| `mod.rs` | `tasty-terminal/src/lib.rs` |
| `vte_handler.rs` | `tasty-terminal/src/vte_handler.rs` |
| `modes.rs` | `tasty-terminal/src/modes.rs` |
| `tests.rs` | `tasty-terminal/tests/` 또는 `src/tests.rs` |

crate 추출 시 끊어야 할 의존:
- `crate::settings::GeneralSettings::detect_shell()` (줄 1092) → crate 인자로 shell 경로를 받도록 변경
- `TerminalEvent.surface_id` (줄 24) → 이 필드는 model 레이어의 관심사이므로 crate 내에서 0으로 설정하고 호출 측에서 할당
