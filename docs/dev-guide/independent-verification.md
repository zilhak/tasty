# 독립 검증 — 개발도 Agent 가 스스로 확인할 수 있어야 한다

> dev-guide 의 **가장 핵심 원칙**. Tasty 정체성인 *동시성*([identity.md](../identity.md))이 **개발 환경 자체에 재귀적으로 적용된 것** 이다.

## 원칙

**Tasty 의 모든 기능은, 그것을 개발하는 AI Agent 가 자기 수정을 독립적으로 띄워 검증할 수 있도록 만들어야 한다.**

Tasty 를 개발하는 환경이 곧 Tasty 다 (dogfooding). 보통 사용자·다른 Agent 는 **release** 빌드를 띄워 작업 중이다. 그 위에서 Agent 가 자기가 고친 것을 확인하려면 **debug** 빌드를 동시에 띄워야 하는데, 둘이 같은 자원(포트·상태 파일)을 공유하면 서로 간섭한다.

그래서 Tasty 는 **debug 빌드와 release 빌드의 환경을 격리** 한다 (0% 충돌은 불가능하지만 최대한). 덕분에 Agent 는 *자신이 release tasty 안에서 동작 중이어도*, 자기 debug 빌드를 따로 띄워 release(= 사용자·다른 작업)와 충돌 없이 검증할 수 있다.

> **Agent 는 이것을 반드시 인지한다**: 내가 tasty 안에서 돌고 있어도, 내 수정은 *별도 debug 인스턴스* 로 띄워 검증한다. 돌아가는 release 를 건드리지 않는다.

## debug ↔ release 격리 (현재 구현)

같은 `~/.tasty/` 아래에서 debug 는 `-debug` suffix 로 분리된다:

| 자원 | release | debug |
|------|---------|-------|
| IPC 포트 파일 | `~/.tasty/tasty.port` | `~/.tasty/tasty-debug.port` |
| scrollback | `~/.tasty/scrollback/` | `~/.tasty/scrollback-debug/` |
| layout | `~/.tasty/layout.json` | `~/.tasty/layout-debug.json` |

- `target/debug/tasty` (debug 바이너리)는 `tasty-debug.port` 를 읽으므로 CLI 조작이 **debug 인스턴스에만** 간다. 사용자의 release 인스턴스는 건드리지 않는다.
- 구현: `crates/tasty-ipc/src/port_file.rs` (`cfg!(debug_assertions)` 분기), `src/store/scrollback.rs`, `src/engine/layout_persistence.rs`.

## 새 기능 추가 시 적용

- 영속 상태(파일/소켓/포트 등)를 새로 추가하면 **debug/release 분리 패턴을 따른다** (debug 는 `-debug` suffix). 안 그러면 debug 검증이 release 데이터를 오염시킨다.
- 동작은 IPC/CLI 로 트리거 가능하게 만든다 (headless 동작-우선) — 그래야 Agent 가 GUI 없이 검증한다. → [identity.md](../identity.md) §2.2.

## 한계 / 주의

- 격리는 **debug ↔ release** 기준이다. 두 debug 인스턴스를 동시에 띄우면 같은 `tasty-debug.port` 를 공유하므로 충돌한다. 여러 Agent 가 병렬로 각자 debug 검증을 해야 하면 별도 격리(분리된 checkout/worktree 운용 등)가 필요하다.

## 관련

- [self-verification.md](self-verification.md) — 실제 검증 절차 (cargo run & + CLI 시나리오)
- [debug-ipc.md](debug-ipc.md) — debug 전용 IPC (사용자 입력 재현, release 미노출)
- [e2e-tests.md](e2e-tests.md) — 테스트 환경 격리 정책
- [identity.md](../identity.md) — 동시성 정체성 (이 원칙의 뿌리)
