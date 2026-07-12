# 크래시 & 에러 진단

tasty 가 죽거나 멈췄을 때 무엇이 어디에 기록되는지, 빌드 모드(release / dev)에 따라 어떤 추가 정보를 어디서 얻는지를 정리한다. 메커니즘은 `src/platform/crash_report.rs`, 부팅 1단계(`boot.rs` → `os::init_n` → `crash_report::init`)에서 설치된다.

## panic 발생 시 (모든 빌드)

부팅 때 설치된 panic hook 이 panic 을 잡아 **두 곳**에 남긴다 (release·dev·dist 공통, panic 전까지 런타임 비용 0):

1. **crash report 파일** — `~/.tasty/crash-reports/crash-<YYYY-MM-DDTHH-MM-SS>.log`
2. **stderr** — `Tasty crashed! Report saved to: <경로>` + `panic: <info>` + 전체 backtrace

### crash report 내용

```
=== Tasty Crash Report ===
Timestamp: 2026-06-17 19:23:07
Version: 0.8.4                      ← CARGO_PKG_VERSION
OS: macos aarch64                  ← std::env::consts::OS / ARCH

=== Panic ===
Location: src/view/main/redraw.rs:142   ← info.location() (있을 때)
Message: called `Result::unwrap()` on an `Err` value: ...
Display: <panic 의 Display 표현>

=== Backtrace ===
   0: std::backtrace::Backtrace::force_capture
   ...
```

backtrace 는 `Backtrace::force_capture()` 로 항상 수집되므로 `RUST_BACKTRACE` 설정 없이도 채워진다.

```bash
ls ~/.tasty/crash-reports/
cat ~/.tasty/crash-reports/crash-*.log
```

## tracing 로그 — `TASTY_LOG`

런타임 로그 레벨은 **`TASTY_LOG`** 환경변수로 제어한다(`RUST_LOG` 아님). 미설정 시 기본 필터: `warn,wgpu_hal=error,wgpu_core=error,naga=error,egui_winit::clipboard=off`.

```bash
TASTY_LOG=debug tasty 2>/tmp/tasty.log                          # 전체 debug
TASTY_LOG=tasty::ipc=debug,tasty::engine=debug tasty 2>/tmp/tasty.log  # 모듈별
```

tracing 출력처는 빌드 모드에 따라 다르다:

| 빌드 | tracing 출력 |
|------|--------------|
| **release / dist** | **stderr 만** |
| **dev** | stderr **+ 파일** `~/.tasty/debug-dev.log` |

`debug-dev.log` 는 dev 빌드에서만 생성되며, 파일 필터는 `debug,wgpu_hal=warn,wgpu_core=warn,naga=warn`(ANSI 없음), **매 실행 시 새로 truncate** 된다. 디렉토리 생성/파일 생성 실패 시 stderr-only 로 자동 폴백.

```bash
cargo run                  # dev
cat ~/.tasty/debug-dev.log # 직전 실행의 전체 debug 로그
```

## 에러 루프 자동 감지 (dev 전용)

dev 빌드는 **`ErrorLoopDetector`** 를 가진다 — 같은 에러 메시지가 **1초 내 100회 이상** 반복되면 의도적으로 panic 을 발생시켜 crash report 로 떨군다. 무한 에러 루프(GPU 재시도 폭주 등)를 영원히 도는 대신 즉시 멈춰 흔적을 남기기 위함.

```
Error loop detected! The following error repeated 100 times in 1s:
<반복된 에러 메시지>
```

호출 지점은 재발 가능성이 높은 루프 — 렌더 루프(`src/view/main/redraw.rs`, 예: "GPU out of memory")와 이벤트 루프(`src/app/event_handler.rs`). 호출 API 는 `crash_report::record_error(msg)` 이며 **release 에서는 no-op**(`#[inline(always)]` 빈 함수)이라 비용·동작이 없다.

## 빌드 모드별 차이 요약

| | release / dist | dev |
|---|---|---|
| crash report 파일 | ✅ | ✅ |
| stderr panic + backtrace | ✅ | ✅ |
| `debug-dev.log` 전체 tracing | ✗ | ✅ |
| 에러 루프 자동 감지 | ✗ (no-op) | ✅ |
| 심볼 / backtrace 품질 | `strip = true` 라 함수명 제한 → 주소만 보일 수 있음 | 미최적화·전 심볼 → 정확한 스택트레이스 |

**release 에서 backtrace 가 주소만 나올 때**: `RUST_BACKTRACE=full tasty` 로 강화하되, strip 된 상태에선 한계가 있다 — 정확한 함수명이 필요하면 dev 빌드로 재현한다.

## panic 없이 멈춤 (무한루프 / 데드락)

panic 이 아니라 hang 이면 **crash report 가 생기지 않는다.** 외부 도구로 붙는다:

```bash
# 멈춘 프로세스
gdb -p $(pidof tasty)
(gdb) thread apply all bt        # 모든 스레드 backtrace
(gdb) thread 3
(gdb) bt full
# 또는 syscall 추적
strace -p <PID> -f -e trace=write,read 2>/tmp/tasty-strace.log
```

- **데드락**: lock 을 잡고 대기하는 지점이 backtrace 에 보인다.
- **무한루프**: 반복되는 호출 패턴이 backtrace 에 보인다.

dev 빌드가 심볼이 온전해 위치 파악이 쉽다.

## 진단 파일 위치 요약

| 파일 / 경로 | 빌드 | 내용 |
|-------------|------|------|
| `~/.tasty/crash-reports/crash-*.log` | 모두 | panic 시 자동(버전·OS·위치·메시지·backtrace) |
| `~/.tasty/debug-dev.log` | dev 만 | 전체 tracing(매 실행 truncate) |
| stderr | 모두 | panic 메시지 + backtrace, `TASTY_LOG` 레벨의 tracing |

## 관련

- [build.md](build.md) — release(`strip = true`) / dev / dist 프로필
- [error-handling.md](error-handling.md) — `Result` 처리·로그 레벨 정책 (애초에 crash 를 줄이는 쪽)
- [self-verification.md](self-verification.md) · [e2e-tests.md](e2e-tests.md) — 재현·검증
- [memory-leak-soak.md](memory-leak-soak.md) — 죽음/멈춤이 아니라 **메모리가 새는** 증상일 때 (soak 테스트 + 계층별 판정)
