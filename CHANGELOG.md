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
- `agent.task_run`(workspace runner thread start/stop/status)이 이제 plugin 에서도 호출 가능하다(`AgentManage` 권한) — 호스트 재시작 후 runner 는 자동으로 켜지지 않으므로(아래 Changed 참조), plugin 이 자기 workspace 의 runner 를 스스로 되살릴 수단이 필요했다. local-only 로 남는 건 `agent.task_set_result` 와(아래 Changed 참조) `agent.task_await` 뿐이다.
- `agent.task_list`/`agent.task_graph` 응답에 `runner: { running, crashed, ready_count, running_count }` 가 동반된다 — runner 가 꺼져 있어도 `ready_count`/`running_count` 는 store 를 직접 조회한 실제 값이라, "할 일은 있는데 아무도 안 돌리고 있다"가 이 응답만으로 드러난다.
- `agent.task_get` 응답에 `awaiting_external: { wait_key, deadline_ms }` 가 추가됐다 — task 가 push 완료 전략(`AwaitExternal` handle)으로 외부 신호를 기다리는 중일 때만 실려, `state: "running"` 만으로는 구분 안 되던 "그냥 실행 중"과 "외부 보고 대기 중"을 구분할 수 있다.
- `tasty agent task-list`/`task-get`/`task-run` CLI 출력이 raw JSON pretty-print 대신 사람이 터미널에서 바로 읽는 텍스트로 렌더된다(`state  id  name` 목록 + `runner: running (ready=N running=M)` 요약 줄, 정지 상태면 재개 커맨드 안내 포함). 다른 `agent` 서브커맨드(barrier/semaphore/lease/rate-limit/task-graph 등)는 기존과 동일하게 JSON.
- `agent.task_delete`(CLI `tasty agent task-delete`) — task 삭제. 참조(`depends_on`/`Fallback.task`/`Reduce.inputs`) 가 있으면 기본 거부하고 참조자 목록을 `error.data.referenced_by` 에 실어 반환(`-32010`), `--cascade` 는 전이적 참조자까지 함께 삭제, `--force` 는 참조 검사만 우회(상태 제약은 못 뚫음). 삭제 금지 상태는 `running` 하나뿐이며 `--cascade`/`--force` 로도 뚫지 못한다(`-32011`).
- `agent.task_purge`(CLI `tasty agent task-purge`) — 상태 이름(`--states`)·경과시간(`--older-than-ms`) 필터 기반 일괄 삭제. `agent.task_delete` 와 동일한 참조 안전 검사를 적용해, 후보 집합 밖에서 여전히 참조되는 task 는 자동으로 보존한다. `--dry-run` 으로 실제 삭제 없이 계획만 확인 가능.
- 부팅 시 정화 경로(`purge_stale_agent_state_on_boot`)에 자동 GC 가 추가됐다 — 상태 무관 + 7일(잠정) 이상 방치된 task 를 `agent.task_purge` 와 동일한 로직으로 정리한다. memory 자체 TTL(`PutOpts.expires_at`) 은 쓰지 않는다.

### Changed

- agent DAG `TaskCommand::Custom.poll`(`PollSpec`)의 `interval_ms` 필드가 생략 가능해졌다 — 기본값 500ms. 이전에는 필수 필드라 생략 시 역직렬화가 실패했다.
- (BREAK) agent DAG `TaskCommand::Reduce.inputs` 가 `depends_on` 과 동일한 암묵적 의존성으로 승격됐다. 이전에는 `depends_on` 없는 `Reduce` task 가 생성 즉시 `ready`→dispatch 되어, 아직 미완인 입력을 `succeeded:false`+`output:null` 로 조용히 수집하고 `Succeeded` 로 마감했다(`all`/`merge_json`/`concat_text` 전략에서 특히 위험 — 실제로 존재하는 값 대신 `null` 을 합성). 이제는 입력이 전부 종결(성공/실패 무관, terminal 상태)될 때까지 `waiting` 을 유지한 뒤 `ready` 로 진행한다. `Reduce.inputs` 는 사이클 검출 대상에도 포함된다.
- agent DAG `TaskCommand::Run`(`agent.task_create`)이 stdout/stderr 를 캡처한다 — 이전에는 자식이 호스트의 stdio 를 그대로 상속해 `result.output` 이 `{"pid": N}` 뿐이었다. 이제 성공 시 `result.output` 에 `stdout`/`stderr` 각각 마지막 64KiB(tail) + `truncated`/`dropped_bytes` 가 담기고, 실패(비0 exit) 시엔 같은 내용이 `result.error` 문자열에 포함된다 — `cargo build` 등을 `Run` 으로 돌렸을 때 실패 원인을 exit code 만으로 추측하지 않아도 된다.
- 호스트 재시작 후 agent task runner 의 재시작 정화(stale semaphore/lease holder 회수, 직전 `Running` task 의 `Failed("host restart")` 마감, persisted handle reload)가 이제 **부팅 시 1회, runner thread 없이도** 라이브 workspace 전부에 적용된다. 이전에는 이 정화가 `agent.task_run --action start` 로 runner 를 수동으로 켜야만 동작해, 재시작 후 `start` 를 안 하면 유령 `Running` task 가 무기한 남았다. **자동 시작 자체는 여전히 도입하지 않는다** — 정화만 부팅 시 돌고, dispatch 재개는 여전히 수동(또는 plugin) `agent.task_run --action start` 가 필요하다.
- `DispatchHandle::AwaitExternal`(push 완료 전략) 의 영속 포맷에 `deadline_ms` 필드가 추가됐다(dispatch 시점 `now + timeout_ms`). `hook_task_waits`(hook_id → task_id) 매핑은 여전히 비영속이라 재시작하면 그 task 는 훅으로는 깨어날 수 없는데, 기존엔 이를 마감할 수단이 전혀 없어 그런 task 가 영구 `Running` 으로 남았다 — 이제 handle 자체의 `deadline_ms` 로 재시작 후에도 만료 판정이 가능하다. 이 필드 도입 이전에 영속된 handle(필드 없음)은 다음 reload 시 `deadline_ms = 0`(즉시 만료)으로 해석돼 `Failed`로 마감된다 — 재시작을 넘겨 살아있던 push-대기 task 가 있었다면, 업그레이드 후 첫 reload 에서 그 task 가 실패 처리된다.
- (BREAK) `agent.task_await` 가 `approval.await` 와 대칭으로 local-only 로 바뀌었다 — plugin 이 호출하면 이제 권한 거부(`-32001`)를 받는다. `task_await` 는 진짜 blocking 이라 plugin SDK 의 단일 워커 스레드를 막을 위험이 있었다(그 plugin 이 다른 host→plugin 요청을 전혀 처리하지 못하게 됨). plugin 은 완료 판정 전략(`[[contributes.completion_strategy]]`)을 선언해 러너가 대신 기다리게 하거나 `task_get` 을 폴링해야 한다. local caller(CLI 등)는 영향 없음.
- (BREAK) `agent.task_await` 의 `timeout_ms` 생략 시 기본값이 **무한 대기 → 10분(600,000ms, 잠정값)** 으로 바뀌었다. 이전엔 `timeout_ms` 를 안 넘기면 terminal 상태까지 무기한 블록됐다 — 그 습관대로 호출하던 코드는 이제 10분 후 `{"outcome":"timed_out"}` 을 받는다. 계속 무한 대기하려면 `timeout_ms: 0` 을 명시해야 한다(CLI `tasty agent task-await --timeout-ms 0`).

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
