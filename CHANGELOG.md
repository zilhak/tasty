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

### Added

- `terminal.state`(CLI `tasty terminal state --surface <child>`) — 자식 단건 상태(`idle`/`needs_input`/`active`/`exited`) 조회. `terminal.children`의 항목별 조회와 달리, registry에서 이미 정리된 surface 도 라이브 트리와 대조해 `"exited"`로 구분한다.
- `claude.state`/`codex.state`(CLI `tasty claude state`/`tasty codex state`) — 위 `terminal.state`를 각 plugin 이 자기 namespace 안에서 위임하는 wrapper. `claude.spawn`/`codex.spawn`에 기본 완료 판정 전략(`[[contributes.completion_strategy]] default_for_methods`)이 새로 연결되어, 이 두 메서드에 한해 DAG `poll` 생략 시 spawn 접수를 완료로 오인하던 기존 동작이 뒤집힌다 — 자식이 실제로 idle/exited 가 될 때까지 `running` 을 유지한다.

### Changed

- agent DAG `TaskCommand::Custom.poll`(`PollSpec`)의 `interval_ms` 필드가 생략 가능해졌다 — 기본값 500ms. 이전에는 필수 필드라 생략 시 역직렬화가 실패했다.
- (BREAK) agent DAG `TaskCommand::Reduce.inputs` 가 `depends_on` 과 동일한 암묵적 의존성으로 승격됐다. 이전에는 `depends_on` 없는 `Reduce` task 가 생성 즉시 `ready`→dispatch 되어, 아직 미완인 입력을 `succeeded:false`+`output:null` 로 조용히 수집하고 `Succeeded` 로 마감했다(`all`/`merge_json`/`concat_text` 전략에서 특히 위험 — 실제로 존재하는 값 대신 `null` 을 합성). 이제는 입력이 전부 종결(성공/실패 무관, terminal 상태)될 때까지 `waiting` 을 유지한 뒤 `ready` 로 진행한다. `Reduce.inputs` 는 사이클 검출 대상에도 포함된다.

### Removed

- (BREAK) `tasty design *` CLI 서브커맨드 11종(`login`/`logout`/`import-session`/`status`/`projects`/`detect`/`probe`/`chat`/`chat-status`/`turn-status`/`protocol`) 및 그 IPC(`design.*`) 전체 제거 — `claude-design` 플러그인이 tasty 본체에서 완전히 빠지며 별도 프로젝트로 분리된다. 대체/alias 없음. 상세: [ADR-0057](docs/adr/0057-remove-claude-design-plugin.md).

### Fixed

- `tasty remote attach --raw`(및 `tasty attach --raw`): 서버/터널 연결이 끊겨도 `--reconnect`(기본 ON) 백오프 재연결이 전혀 동작하지 않던 결함 수정. raw 브리지가 종료 사유와 무관하게 `process::exit(0)` 으로 프로세스를 죽여 재연결 판단 지점(`AttachExit::Disconnected`)에 도달하지 못했다 — 이제 mirror-dump 와 동일하게 채널 기반으로 종료 사유를 구분해 정상 반환한다.
- 완료 판정 전략(`[[contributes.completion_strategy]]`)의 `default_for_methods`/`poll_method` namespace 검증이 plugin owner 를 매니페스트의 reverse-DNS id(예: `com.tasty.claude`)로 비교해, 실제 IPC dispatch 접두어(`claude`)와 절대 일치하지 않아 plugin 소유 전략이 등록 시점에 전부 조용히 drop 되던 결함 수정 — 이제 그 plugin 이 실제로 선언한 `ipc_namespace` 접두어와 비교한다.
- `agent.task_create` 가 `depends_on` 과 달리 `OnFailure::Fallback{task}`/`TaskCommand::Reduce.inputs` 가 가리키는 task id 의 존재를 검증하지 않던 결함 수정. 미존재 fallback 은 main 실패 시 조용히 무시되어 그 main 에 의존하는 downstream 이 영구 `waiting` 에 빠졌고, 미존재 reduce 입력은 dispatch 시점에야 뒤늦게 실패했다. 이제 둘 다 생성 시점에 `-32602` 로 거부된다. 검증 도입 이전에 이미 저장된 dangling 참조는 마이그레이션하지 않는다(신규 생성만 차단) — 그런 참조가 실패 전이를 타면 `tracing::warn!` 을 남긴다.

## [0.9.7] - 2026-07-15

많은 변경이 있었음(누적된 릴리스 갭).

## [0.9.6] - 2026-07-15

많은 변경이 있었음(누적된 릴리스 갭).

## [0.9.4] - 2026-07-14

많은 변경이 있었음(누적된 릴리스 갭).
