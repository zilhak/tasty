# Changelog

본 문서는 사용자(AI 에이전트 포함)가 의존하는 표면 — CLI 명령, IPC 메서드, 매니페스트 스키마, plugin 인터페이스 — 의 변경만 기록한다. 내부 refactor·테스트·문서는 `git log`를 참조.

형식: [Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/). 버전: [SemVer](https://semver.org/lang/ko/).

각 변경은 다음 카테고리 중 하나에 속한다:

- `Added` — 새 기능, 새 메서드/명령
- `Changed` — 동작 변경 (BREAK는 머리에 `(BREAK)` 표기)
- `Deprecated` — 폐기 예정, 아직 동작은 함
- `Removed` — 제거된 기능
- `Fixed` — 버그 수정

자세한 안정성 정책·break 분류·deprecation 절차는 [`docs/dev-guide/api-conventions.md`](docs/dev-guide/api-conventions.md) 의 "안정성 정책" 절 참조.

## [Unreleased]

### Changed
- **(BREAK) `tasty claude spawn`/`tell` 이 더 이상 동기 블록하지 않는다** — 기존엔 `--no-wait` 를 주지 않으면 child 가 idle/needs_input/exited 에 도달할 때까지 CLI 프로세스가 block(무한 대기 가능)했다. 이제 항상 즉시 반환하고, 대상이 완료되면 caller surface(spawn/tell 을 호출한 surface)에 완료 메시지가 1회성 surface hook 으로 자동 주입된다. `--no-wait`/`--timeout` 플래그는 더 이상 의미가 없으므로 함께 제거됐다(전달해도 무시되지 않고 unknown-flag 에러). 0.x 정책상 직접 대체 — 호환 alias 없음.
- **(BREAK) `tasty codex spawn`/`tell` 이 더 이상 동기 블록하지 않는다** — claude와 동일한 방향. 호출 즉시 반환하고, 대상이 idle 또는 exited 에 도달하면 caller surface 에 1회성 알림 메시지가 자동 주입된다(`codex-idle`/`process-exit` hook → `codex notify-caller`). `tell` 은 신규 `--caller-surface` flag 로 알림 받을 surface 를 지정(생략 시 호출자의 `TASTY_SURFACE_ID`).
- codex 가 기동하는 모든 명령(`spawn`/`launch`/`reboot` 의 resume)에 `--dangerously-bypass-hook-trust` 를 추가 — 사용자가 `/hooks` 로 수동 승인하지 않아도 tasty 가 심은 hook 이 항상 fire 된다.

### Removed
- **(BREAK) `claude.wait`/`claude.wait_by_surface`/`claude.wait_any` IPC 메서드 + `tasty claude wait`/`wait-any` CLI 제거** — 위 이벤트 기반 알림으로 대체됐다. 폴링 기반 대기가 필요하면 더 이상 이 plugin 이 제공하지 않는다.
- **(BREAK) `claude-child-idle`/`claude-child-needs-input` surface hook 이벤트 제거** — spawn 시점에 등록된 parent-child 관계를 기준으로 매번 parent 에 fan-out 하던 구식 메커니즘. 신규 1회성 알림(위)이 caller 기준·1회성·`tell` 의 임의 surface 대상까지 커버하는 더 일반적인 방식으로 완전히 대체한다.
- `tasty codex wait` (BREAK) — 동기 폴링 명령 제거. `spawn`/`tell` 의 자동 알림으로 대체.
- **(BREAK) `terminal.wait` IPC 메서드 + `tasty terminal wait`/`spawn --wait`/`tell --wait` CLI 제거** — claude/codex 의 동기 wait 를 이벤트 기반 알림으로 대체하면서 `terminal.wait` 의 마지막 호출자가 사라졌다. `terminal.set_state`(에이전트 hook 진입점)는 영향 없이 유지된다.

### Added
- **`claude.notify_done` IPC / `tasty claude notify-done` CLI (내부용)** — `spawn`/`tell` 이 등록하는 1회성 알림 hook(`claude-idle`/`needs-input`/`process-exit`)이 fire 될 때 실행되어, caller surface 에 완료 메시지를 전달하고 남은 형제 hook 을 정리한다.
- `crates/tasty-plugin-codex/tasty-plugin.toml` `[[contributes.hook_events]]` 에 `codex-idle` 신규 선언.
- `tasty codex notify-caller` (내부용) — `spawn`/`tell` 이 등록한 완료 알림 hook 이 fire 될 때 실행되는 명령.
