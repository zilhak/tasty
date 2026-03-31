# Crash & 에러 진단 가이드

Tasty에서 에러가 발생했을 때 원인을 추적하는 방법을 빌드 모드별로 정리한다.

## Release 빌드

### Crash Report 자동 생성

Panic 발생 시 `~/.tasty/crash-reports/crash-YYYY-MM-DDTHH-MM-SS.log` 파일이 자동 생성된다.

```
=== Tasty Crash Report ===
Timestamp: 2026-03-31 19:23:07
Version: 0.1.0
OS: linux x86_64

=== Panic ===
Location: src/main.rs:123
Message: called `Result::unwrap()` on an `Err` value: ...

=== Backtrace ===
   0: std::backtrace::Backtrace::force_capture
   1: tasty::crash_report::init::{{closure}}
   ...
```

**확인 방법:**

```bash
ls ~/.tasty/crash-reports/
cat ~/.tasty/crash-reports/crash-*.log
```

### 환경변수로 상세 로그 활성화

```bash
# 기본 로그 레벨 변경
TASTY_LOG=debug tasty 2>/tmp/tasty-debug.log

# 특정 모듈만 상세 로그
TASTY_LOG=tasty::ipc=debug,tasty::engine=debug tasty 2>/tmp/tasty-debug.log
```

### 스택트레이스 강화

Release 빌드는 `strip = true`이므로 심볼이 없다. 더 정확한 스택트레이스가 필요하면:

```bash
# RUST_BACKTRACE와 함께 실행
RUST_BACKTRACE=full tasty
```

심볼이 strip된 상태에서는 주소만 보일 수 있다. 정확한 함수명이 필요하면 debug 빌드를 사용한다.

### 무한루프/데드락 (panic 없이 멈춤)

Release 빌드에서는 crash report가 생성되지 않는다. 다음 방법을 사용한다:

```bash
# 1. 멈춘 프로세스의 PID 확인
pidof tasty

# 2. strace로 현재 상태 확인
strace -p <PID> -f -e trace=write,read 2>/tmp/tasty-strace.log

# 3. gdb attach (심볼 없어도 대략적인 위치는 보임)
gdb -p <PID>
(gdb) thread apply all bt
```

## Debug 빌드

Debug 빌드는 Release에서 제공하는 모든 기능에 더해 추가 진단 도구를 제공한다.

### 상세 파일 로깅

Debug 빌드로 실행하면 `~/.tasty/debug.log`에 모든 tracing 이벤트가 자동 기록된다.

```bash
cargo run
# 에러 발생 후
cat ~/.tasty/debug.log
```

로그 레벨은 기본 `debug`이며, wgpu 관련은 `warn`으로 필터링된다. 매 실행 시 파일이 초기화된다.

### 에러 루프 자동 감지

동일 에러가 1초 내에 100회 이상 반복되면 자동으로 panic이 발생한다:

```
Error loop detected! The following error repeated 100 times in 1s:
<에러 메시지>
```

이 panic은 crash report로 기록되므로 `~/.tasty/crash-reports/`에서 확인할 수 있다.

### gdb로 정확한 디버깅

Debug 빌드는 최적화가 없어서 함수가 인라인되지 않고, 모든 심볼이 포함되어 정확한 스택트레이스를 제공한다.

```bash
# 1. debug 빌드로 실행
cargo run

# 2. 무한루프/데드락 걸리면 다른 터미널에서
gdb -p $(pidof tasty)

# 3. 모든 스레드의 backtrace 확인
(gdb) thread apply all bt

# 4. 특정 스레드로 전환
(gdb) thread 3
(gdb) bt full
```

**데드락이면**: lock을 잡고 대기하는 지점이 backtrace에 나타난다.
**무한루프이면**: 반복되는 함수 호출 패턴이 backtrace에 나타난다.

## 진단 파일 위치 요약

| 파일 | 빌드 | 설명 |
|------|------|------|
| `~/.tasty/crash-reports/crash-*.log` | 모두 | Panic 시 자동 생성 |
| `~/.tasty/debug.log` | Debug만 | 전체 tracing 이벤트 |
| stderr | 모두 | panic 메시지 + backtrace |
