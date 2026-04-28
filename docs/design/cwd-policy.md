# CWD 정책

## 개요

터미널의 현재 작업 디렉토리(CWD)를 감지하여 새 탭/split 생성, 레이아웃 저장, 닫힌 항목 복원, 터미널 링크 해석 등에서 활용하는 메커니즘.

CWD 정책은 **OS별로 다르게 운영**한다. 환경마다 OSC 7 송신 여부와 CWD 조회 비용이 크게 달라서 통일된 폴링 정책을 적용할 수 없기 때문이다.

## CWD 소스 (우선순위)

1. **OSC 7** (CurrentWorkingDirectory): 쉘이 프롬프트마다 `\e]7;file://hostname/path\e\\`을 보내면 즉시 `cached_cwd`에 반영. 비용 0.
2. **OS 레벨 폴링**: 쉘이 OSC 7을 보내지 않는 환경의 폴백. 백그라운드에서 주기적으로 PID의 CWD를 조회하여 `cached_cwd`를 갱신.

OSC 7이 오면 그 값이 캐시를 덮어쓰므로, 두 소스가 공존해도 문제없다.

## OS별 정책

| OS | OS 레벨 조회 | 비용 | 폴링 | 근거 |
|----|--------------|------|------|------|
| Linux | `/proc/{pid}/cwd` readlink | ~0ms | **사용** | 폴링 비용이 사실상 0이라 OSC 7 미송신 셸이 있어도 안전한 폴백. |
| macOS | `lsof -p {pid} -Fn -a -d cwd` | ~1-5ms | **사용 (필수)** | macOS 기본 zsh가 OSC 7을 송신하지 않아 폴백이 실제로 필요. lsof는 콘솔창을 띄우지 않고 가벼움. |
| Windows | (없음) | — | **미사용** | 다른 프로세스의 CWD를 얻는 표준 API가 없다. WMI는 CWD 필드 자체가 없고, PowerShell 호출은 콘솔창을 띄우는 무거운 동작이라 폴링에 부적합. git bash·PowerShell 7+(oh-my-posh)·Windows Terminal cmd 모두 OSC 7을 송신하므로 그것에 의존한다. |

구현: `crates/tasty-terminal/src/cwd.rs`의 `get_cwd_of_pid(pid)`. Windows에서는 항상 `None` 반환.

## 폴링 전략 (macOS/Linux): 고정 부하 라운드 로빈 + 포커스 우선

### 목표

- 터미널 수에 관계없이 **시스템 부하 고정** (50ms당 OS 호출 1회)
- 포커스 터미널은 **최대 100ms 내 CWD 반영**

### 구현

50ms 간격으로 `AppEvent::CwdPoll` 이벤트를 발생시키는 백그라운드 스레드가 있다 (Windows에서는 스레드 자체를 띄우지 않음).

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

## Windows에서 셸이 OSC 7을 안 보내면?

`cached_cwd`가 비어 있는 상태로 유지되며, 새 터미널 분할 시 부모 CWD 상속이 동작하지 않는다. 사용 중인 셸이 OSC 7을 송신하도록 프롬프트를 설정하면 해결된다 (예시: bash의 `PROMPT_COMMAND`에 `printf '\033]7;file://%s%s\033\\' "$HOSTNAME" "$PWD"` 추가).

향후 정말로 PID→CWD가 필요한 케이스(분할/저장 등)에서 `NtQueryInformationProcess` + PEB 읽기 기반의 unsafe FFI fallback을 **이벤트 시점에만 1회** 호출하는 방식으로 추가할 수 있다. 현재는 폴링 자체가 부적합한 비용이라 미구현.

## 관련 코드

| 파일 | 역할 |
|------|------|
| `crates/tasty-terminal/src/cwd.rs` | OS 레벨 PID → CWD 조회. Linux/macOS 분기. Windows는 `None`. |
| `crates/tasty-terminal/src/lib.rs` | `Terminal::get_cwd()`, `set_cached_cwd()` |
| `crates/tasty-terminal/src/vte_handler.rs` | OSC 7 수신 시 `cached_cwd` 즉시 갱신 (모든 OS) |
| `src/engine_state.rs` | `poll_one_cwd_round_robin()`, `poll_one_cwd_focused()` (macOS/Linux 전용) |
| `src/event_handler.rs` | `poll_one_terminal_cwd()` 토글 로직 (macOS/Linux 전용) |
| `src/main.rs` | `AppEvent::CwdPoll` + 50ms 폴링 스레드 (macOS/Linux 전용) |
