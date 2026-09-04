# 에러 처리 정책

`Result` 를 무음 무시하면 실패가 흔적 없이 사라져 회귀 추적이 불가능해진다. **모든 `Result` 는 처리하거나 로그를 남긴다.** 강제 정책은 [`../../CLAUDE.md`](../../CLAUDE.md) "에러 처리".

## 원칙

`Result` 를 `let _ =` 로 무시하지 않는다. 무시는 *명시적 정책 결정* 이어야 하고 그 이유가 주석으로 남는다.

```rust
// ✅ 실패 시 로그 (기본 패턴)
if let Err(e) = self.state.split_surface(SplitDirection::Vertical) {
    tracing::warn!("split_surface failed: {e}");
}

// ✅ 상위로 전파 (호출자가 처리 가능)
self.state.split_surface(SplitDirection::Vertical)?;

// ❌ 무음 무시 — 금지
let _ = self.state.split_surface(SplitDirection::Vertical);
```

## 로그 레벨

| 레벨 | 시점 |
|------|------|
| `tracing::error!` | 복구 불가, 사용자 작업이 의미를 잃음 (설정 저장 실패, layout 파싱 실패) |
| `tracing::warn!` | 무시해도 동작은 계속되는 부분 실패 (옵션 hook 실패, 텔레메트리 전송 실패) |
| `tracing::debug!` | 정상 흐름의 한 분기로 실패가 예상됨 (optional feature 미설치) |

기준선: 사용자가 "방금 왜 안 됐지?" 라고 물었을 때 답을 찾을 로그가 남아 있어야 한다.

## 의도적 무시

진짜로 무시해야 하는 극소수만 `let _ =` 를 허용하되 **왜 무시하는지 한 줄 주석**을 단다. 근거 없는 `let _ =` 는 리뷰에서 차단.

```rust
// 채널 receiver 가 이미 drop 된 정상 종료 케이스 — 송신 실패 무시.
let _ = tx.send(msg);
```

### 주석 위치

사유 주석은 **그 문장 옆에서 읽혀야** 한다. 인정하는 위치는 셋이다.

| 위치 | 예 |
|------|-----|
| 같은 줄 | `let _ = out.flush(); // 직후 exit — 전달할 호출자가 없다.` |
| 바로 윗줄 (빈 줄과 `#[..]` 속성은 사이에 있어도 된다) | `// 이유`<br>`let _ = f();` |
| 문장 범위 안 또는 바로 다음 줄 | 멀티라인 호출의 인자 사이, rustfmt 가 trailing 주석을 밀어낸 다음 줄 |

**블록 상단에 한 번 적은 설명은 사유로 인정하지 않는다.** 몇 줄 위의 주석을 인정하려면
"어디까지 거슬러 올라가는가" 를 정해야 하는데, 함수 doc 주석까지 닿을 만큼 넓히면 사실상
모든 `let _ =` 가 통과해 검사가 검사를 그만둔다. 위에 이미 적었더라도 한 줄을 더 적는다.

전수 강제는 [`tests/let_underscore_documented.rs`](../../tests/let_underscore_documented.rs)
가 한다(`cargo test --workspace`). 텍스트 스캔은 타입을 알 수 없으므로 규칙은 `Result` 만이
아니라 **모든 `let _ =`** 에 적용된다 — 변수 바인딩 억제(`let _ = path;`)도 "왜 여기서 안
쓰는가" 가 궁금한 지점이라 한 줄 주석이 손해가 아니다.

테스트 코드는 대상이 아니다 — `tests/`·`benches/` 디렉토리와 `#[cfg(test)]` / `#[test]`
아이템 본문. 테스트에서 값을 버리는 것은 대개 의도가 자명하고, 여기까지 강제하면
통과시키기 위한 형식적 주석만 늘어난다. 정책이 지키려는 것은 **프로덕션에서 조용히
사라지는 실패**다.

**다만 그 제외에는 비용이 있다.** `crates/tasty-gallery/tests/specimen_smoke.rs` 의
`run_frames` 는 `let _ = ctx.run(..)` 로 egui 의 `FullOutput` 을 버렸는데, 그 안에 담긴
것이 이 스모크가 확인하려던 레이아웃 결과였다 — 값을 버린 탓에 테스트가 초록으로
통과하면서도 레이아웃 결함을 못 잡고 있었다. **값을 버리는 것이 검증 자체를 무력화한
실례**다. 그래도 강제 범위를 테스트까지 넓히지 않는 이유는, 형식 주석을 요구해도 이
결함은 안 잡히기 때문이다 — "왜 버리는가" 를 한 줄 적는 것과 "버리면 안 되는 값이었다"
를 알아채는 것은 다른 일이다. 테스트에서 값을 버릴 때는 주석보다 먼저 **그 값이 검증의
일부가 아닌지**를 본다.

`.githooks/pre-commit` C.6 이 staged diff 의 추가 라인에 대해 같은 판정을 먼저 한다. 훅은
같은 줄 · 바로 윗줄 · 바로 다음 줄만 보므로 위 표보다 좁다 — 훅이 통과시킨 코드를 CI 가
떨어뜨리는 방향은 생기지 않는다.

두 검사 다 렉서가 아니라 텍스트 스캔이라 한계가 있다. 문장 안에 주석이 아닌 `//` 가
있으면(URL 문자열) 사유 주석으로 오인해 넘어가고, 반대로 문자열 리터럴 안의 금지 형태는
코드로 오인한다. 앞은 안전한 방향의 미탐이라 그대로 두고, 뒤는 가드의 ALLOWLIST 에
`(경로, 조각)` 으로 등록해 처리한다 — **파일을 통째로 면제하지 않는다.** 한계 전문은
가드 파일 머리 주석에 있다.

## stdout 쓰기 (CLI 클라이언트)

`println!` / `print!` 는 stdout 쓰기 실패를 panic 으로 승격한다. 읽는 쪽이 파이프를 먼저 닫으면
(`tasty list tree | head -1`, `| true`) EPIPE 가 돌아오고, 그 panic 이 종료 코드 101 + 가짜
crash report 가 된다. Rust 런타임은 SIGPIPE 를 무시하도록 두고 Windows 에는 SIGPIPE 가 없으므로,
**stdout 쓰기도 `Result` 로 받아 처리한다** — 근거와 대안은 [ADR-0101](../adr/0101-cli-stdout-broken-pipe-exit-zero.md).

- `crates/tasty-cli` 는 stdout 에 **`crate::out` 의 `outln!` / `out!`** 로만 쓴다(`println!` /
  `print!` 금지 — `tests/cli_stdout_broken_pipe.rs` 가 소스 스캔으로 강제). 값은
  `anyhow::Result<()>` 라 `?` 로 올린다. 개행 없는 출력 뒤의 flush 는 `crate::out::flush()?`,
  stdout 에 직접 쓰는 외부 코드(clap `print_help`)의 `io::Result` 는 `crate::out::from_io(..)?`.
- `ErrorKind::BrokenPipe` 는 `StdoutClosed` 로 구분돼 호출 스택을 타고 올라오고, CLI 진입점
  (`run_client` / `print_augmented_help` / `print_command_tree` / `try_run_plugin_cli`)이
  `quiet_if_stdout_closed` 로 **종료 코드 0** 으로 접는다. 실패가 아니라 "더 쓸 곳이 없다" 는
  신호다. 그 외 stdout 오류(EIO / ENOSPC 등)는 일반 에러로 전파된다.
- 폴링/follow 루프에서 읽는 쪽이 사라졌을 때 루프를 빠져나오는 지점은 **다음 출력**
  (`outln!`/`out!` 의 write)이다. `crate::out::flush()?` 는 버퍼에 남은 바이트가 있을 때만
  write(2) 를 내므로 **빈 버퍼 flush 는 EPIPE 를 감지하지 못한다** — flush 를 파이프 생존
  프로브로 쓰지 않는다. 출력이 더 없으면 `tail -f | head -1` 처럼 계속 대기한다.
- host(GUI / headless)는 stdout 에 쓰지 않는다(pre-commit C.11 이 `println!` 을 막는다). 이
  정책은 CLI 클라이언트 갈래에만 적용되고 `Routed::Gui` 의 동작은 바뀌지 않는다.
- 예외: `local/attach.rs` raw bridge 는 `std::io::stdout()` 핸들에 best-effort 미러하고 결과를
  주석과 함께 무시한다 — stdout 이 닫혀도 attach 세션은 계속돼야 한다.

```rust
// ✅ CLI 출력
outln!("{}", serde_json::to_string_pretty(&value)?)?;
out!("{chunk}")?;
crate::out::flush()?;

// ❌ panic 승격 — EPIPE 가 crash report 가 된다
println!("{}", serde_json::to_string_pretty(&value)?);
```

## 창·GPU·엔진·스레드 생성 실패

생성 실패는 패닉하지 않는다. 이미 터미널 세션이 떠 있는 상태에서 패닉하면 그 창 하나가
아니라 **실행 중인 모든 창의 작업**이 사라진다. 결정 전문과 근거는
[ADR-0117](../adr/0117-window-and-modal-creation-failure-policy.md).

| 지점 | 처리 |
|------|------|
| 부팅 창 생성 · 부팅 엔진 생성 · GPU 어댑터 부재 | 진단 3줄을 `tracing::error!` 한 이벤트로 내고 `exit(1)`. `eprintln!` 은 파일 로그에 안 남아 쓰지 않는다 |
| 부팅의 그 외 GPU 실패 | 패닉 유지 — 환경 문제가 아니라 버그이므로 크래시 리포팅 경로에 남긴다 |
| 새 창 · 설정 · 플러그인 모달 | 그 창만 취소하고 살아 있는 메인 창에 안내. 안내 문구는 지점별 i18n 키 |
| 종료 확인 모달 | 확인을 건너뛰고 `begin_shutdown()`. 생략 사실을 toast + `error!` 로 알린다 |
| 호스트 스레드 spawn | 에러 반환(`ObserverError::ThreadSpawn`) 또는 로그 후 미등록 |
| plugin 프로세스 스레드 spawn | 패닉 유지 — 폭발 반경이 그 plugin 프로세스로 한정된다 |

**안내 채널은 요청 origin 이 가른다.** 사용자 조작(메뉴 · 단축키 · dock · tray)발 실패는
`InfoModal`, 에이전트 IPC(`window.create`)발 실패는 **toast** 다. `InfoModal` 은 포커스를
가져가므로, 에이전트 행동의 부수효과가 사용자 포커스에 닿지 않는다는 핵심 원칙 1 을
어기게 된다.

이 경로들은 winit `ActiveEventLoop` 가 있어야 돌아가 행동 테스트로 감쌀 수 없다 —
`tests/no_panic_in_window_creation.rs` 가 소스 형태로 패닉 재유입을 막는다.

## 로그 메시지 작성

**무엇이** 실패했는지 + **원인** + (가능하면) **영향**. 변수 보간으로 컨텍스트를 담는다.

```rust
tracing::warn!("failed");                                    // ❌ 컨텍스트 없음
tracing::warn!("hook {hook_id} failed for surface {surface_id}: {e}"); // ✅
```

## anyhow / thiserror

에러 타입 정의·`?` 전파·context 첨부 등 `anyhow`/`thiserror` 사용법은 상류 문서를 따른다 — 본 문서는 tasty 의 *정책* 만 다룬다.
