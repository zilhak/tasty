# CWD 폴링 설계

## 개요

터미널의 현재 작업 디렉토리(CWD)를 감지하여 새 탭/split 생성 시 CWD를 상속하는 메커니즘.

## CWD 소스 (우선순위)

1. **OSC 7** (CurrentWorkingDirectory): 쉘이 프롬프트마다 `\e]7;file://hostname/path\e\\`을 보내면 즉시 `cached_cwd`에 반영. 비용 0.
2. **OS 레벨 폴링**: 쉘이 OSC 7을 보내지 않는 환경(macOS zsh 기본 설정 등)을 위한 폴백. 백그라운드에서 주기적으로 PID의 CWD를 조회하여 `cached_cwd`를 갱신.

OSC 7이 오면 그 값이 캐시를 덮어쓰므로, 두 소스가 공존해도 문제없다.

## OS 레벨 CWD 조회 방법

| 플랫폼 | 방법 | 비용 |
|--------|------|------|
| Linux | `/proc/{pid}/cwd` readlink | ~0ms |
| macOS | `lsof -p {pid} -Fn -a -d cwd` | ~1-5ms |
| Windows | PowerShell `Get-CimInstance Win32_Process` | ~100ms+ |

구현: `crates/tasty-terminal/src/cwd.rs`의 `get_cwd_of_pid(pid)`.

## 폴링 전략: 고정 부하 라운드 로빈 + 포커스 우선

### 목표

- 터미널 수에 관계없이 **시스템 부하 고정** (50ms당 lsof 1회)
- 포커스 터미널은 **최대 100ms 내 CWD 반영**

### 구현

50ms 간격으로 `AppEvent::CwdPoll` 이벤트를 발생시키는 백그라운드 스레드가 있다.

매 이벤트마다 **순차 갱신**과 **포커스 갱신**을 번갈아 실행:

```
tick 0: 순차 갱신 (라운드 로빈으로 다음 터미널 1개)
tick 1: 포커스 갱신 (현재 포커스된 터미널)
tick 2: 순차 갱신
tick 3: 포커스 갱신
...
```

### 라운드 로빈 순회

`last_polled_id`를 기억하고, 전체 터미널 목록에서 이 ID보다 큰 첫 번째 터미널을 선택. 더 큰 ID가 없으면 목록 첫 번째로 돌아간다.

터미널이 추가/삭제되어도 안전하다:
- 삭제된 ID는 목록에 없으므로 자동으로 건너뜀
- 중간 ID가 비어있어도 "보다 큰 첫 번째"로 넘어감

### 갱신 주기 (예시)

| 터미널 수 | 포커스 터미널 | 비포커스 터미널 |
|-----------|-------------|---------------|
| 1개 | 100ms | - |
| 10개 | 100ms | ~1초 |
| 20개 | 100ms | ~2초 |
| 50개 | 100ms | ~5초 |

포커스 터미널은 항상 100ms. 비포커스는 `터미널 수 × 100ms`.

## 관련 코드

| 파일 | 역할 |
|------|------|
| `crates/tasty-terminal/src/cwd.rs` | OS 레벨 PID → CWD 조회 |
| `crates/tasty-terminal/src/lib.rs` | `Terminal::get_cwd()`, `set_cached_cwd()` |
| `crates/tasty-terminal/src/vte_handler.rs` | OSC 7 수신 시 `cached_cwd` 즉시 갱신 |
| `src/engine_state.rs` | `poll_one_cwd_round_robin()`, `poll_one_cwd_focused()` |
| `src/event_handler.rs` | `poll_one_terminal_cwd()` - 토글 로직 |
| `src/main.rs` | `AppEvent::CwdPoll` + 50ms 폴링 스레드 |
