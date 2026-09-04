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

- 사용자 언어팩 발견·선택 — `~/.tasty/lang/<code>/pack.toml`(디렉토리 + 매니페스트)이 하나의 언어팩이다. `[meta] name`(콤보 표시 이름, 없으면 코드)과 **필수** `[font]`(`builtin = true` / `file` / `family` / `candidates` 중 하나)를 갖고, 나머지 문자열 키는 영어 베이스 위에 overlay 된다(빈 문자열 값은 번역 없음으로 보고 영어로 폴백). Settings › General 의 언어 콤보가 내장 `en`/`ko`/`ja` + 발견된 팩 전부를 노출한다(`tasty_i18n::available_languages`). 내장 `lang/{en,ko,ja}.toml` 에도 `[meta] name` 이 생겼다.
- `surface.attention.clear`(CLI `tasty surface attention clear --surface <id> [--kind completion|needs_input]`) — attention(주의 환기) 해제. 지금까지 attention 은 `surface.completion` 으로 발동만 가능하고 해제 수단이 IPC/CLI 에 없었다(해제 producer 두 개가 전부 GUI 로컬 사건 — 실 렌더 포커스, 알림 패널 읽음). `--kind` 를 주면 현재 기록된 kind 가 그 값일 때만 지운다(생략 = kind 무관, 알 수 없는 값은 거절). attention 이 없던 surface 도 성공하며(idempotent) 응답 `cleared`/`previous_kind` 가 실제 결과를 알린다. 존재하지 않는 surface · **하드 점유(원격 attach) 중인 surface** · **mirror surface** 는 명시적 에러 — 뒤의 둘은 그 attention 의 소유자가 다른 인스턴스다(각각 ADR-0040 · ADR-0098/0104). 미러 사용자가 그 화면을 실제로 보고 확인한 해제는 종전대로 소유 인스턴스로 전달된다. 권한 `Notification`.
- `surface.attention.get`(CLI `tasty surface attention get --surface <id>`) — 그 surface 에 기록된 attention kind(`"completion"`/`"needs_input"`/`null`) 조회. 읽기 전용이라 mirror·점유 중에도 허용. 권한 `Notification`.

### Changed

- `~/.tasty/lang/<code>.toml` 단일 파일은 이제 **내장 코드(`en`/`ko`/`ja`)의 오버라이드 전용**이다 — 내장이 아닌 코드의 단일 파일은 팩으로 인정하지 않고 `tracing::warn!` 후 무시한다(`<code>/pack.toml` 로 옮겨야 한다).
- `general.language` 가 팩 없는/무효한 코드를 가리키면 영어로 폴백하고 부팅 후 경고 토스트 1회(요청 코드 + 기대 경로)를 띄운다 — headless/CLI 는 `tracing::warn!` 한 줄. 설정값은 덮어쓰지 않으며, `current_language()` 와 plugin 에 전달되는 `TASTY_LOCALE` 은 실제 적용 언어(`en`)를 싣는다. `tasty_i18n::init` 이 `LoadReport` 를 돌려준다(이전에는 반환값 없음).
- 설정 콤보에서 현재 설정값이 목록에 없으면(팩 삭제 등) `<code> (not found)` 행으로 유지한다 — 콤보를 열고 닫아도 값이 바뀌지 않는다.

### Fixed

- 진단 로그(`tracing`)가 **stdout** 으로 나가 CLI 출력을 오염시키던 문제. `tasty list tree | jq .` 처럼 stdout 을 기계가 파싱하는 경로에서, 경고가 한 줄이라도 나면 JSON 앞에 섞여 파싱이 깨졌다 — 로그 레이어의 writer 가 stderr 가 아니라 기본값(stdout)이었다. 이제 stderr 로 나간다(파일 로그 레이어는 종전과 동일).
- 에이전트의 close 가 사용자가 보고 있던 워크스페이스 · 탭 · pane 을 옮기던 문제(3계층 전부). 활성 포인터 중 둘(`active_workspace` · `active_tab`)이 인덱스라 **앞쪽** 원소가 닫히면 인덱스는 그대로인 채 가리키는 대상이 바뀌었고, pane 은 닫힌 pane 이 포커스였는지 보지 않고 무조건 첫 pane 으로 포커스를 재배정했다. 그래서 에이전트가 release IPC `surface.close` 로 자기 워크스페이스를 정리하는 것만으로 사용자 화면이 흔들렸다(불가침 원칙 1 위반). 이제 세 계층 모두 **제거 위치를 기준으로 보정**해 같은 대상을 계속 가리키고, 시야는 사용자가 보던 대상 자체가 사라졌을 때만 움직인다. 같은 보정이 사용자 경로(컨텍스트 메뉴로 앞쪽 탭/워크스페이스 닫기)·`surface.move`·원격 attach forward 에도 적용된다. mirror 워크스페이스 teardown 은 이미 같은 보정을 하고 있었고, 이번엔 같은 헬퍼로 수렴시켰다 — 거기서 순증한 것은 카테고리 착지점 보정이다. 카테고리 quick-switch 착지점(전역 인덱스)도 함께 보정된다. 근거 ADR-0113.
- headless 인스턴스(`--headless`)에서 `surface.completion` 이 아무 효과가 없던 문제. Intent 큐를 drain 하는 지점이 gui 빌드 전용이고 headless IPC 펌프는 그 큐를 읽지 않아, 핸들러가 enqueue 한 발동 요청을 처리할 주체가 없었다 — 이제 IPC 핸들러가 대상 engine 에 직접 적용한다(응답 계약은 불변). 같은 이유로 새 `surface.attention.{get,clear}` 도 headless 에서 동작한다.
- headless 인스턴스(`--headless` / `--no-default-features` 빌드)에서 `surface.set_mark` · `notification.create` · `settings.set_remote_transfer` 가 `ok` 를 회신하고도 아무 상태도 바꾸지 않던 문제. 위 attention 축과 같은 원인이지만 이쪽은 핸들러 직접 적용 대상이 아니어서 남아 있었다 — 예로 `set mark` 직후의 `read since-mark` 가 mark 를 무시하고 스크롤백 전체를 돌려줬다. 이제 headless 도 요청 응답 전에 Intent 큐를 비워 적용하며, 큐 길이는 요청 수와 무관하게 유계다. 근거 ADR-0111.
- host 가 plugin 프로세스에 `TASTY_LOCALE` 을 실제로 채워 보낸다 — 이전에는 host 어디서도 이 env 를 set 하지 않아 `general.language = "ko"`/`"ja"` 여도 plugin UI(클립보드 뷰어 · git 뷰어 등)가 항상 영어였다(셸에서 직접 `export TASTY_LOCALE=…` 한 경우에만 우연히 동작). 이제 부팅 시 `general.language` 를 host 프로세스 env 에 set 해 모든 plugin 이 상속하며, 셸의 export 값은 설정에 덮인다. 값은 spawn 시점 고정 — 언어 변경은 재시작 후 반영.
- CLI 클라이언트가 stdout 파이프 조기 종료(EPIPE — `tasty list tree | head -1`, `| true` 등 읽는 쪽이 먼저 닫힘)를 만나면 `failed printing to stdout` 으로 panic 해 종료 코드 101 을 내고 `~/.tasty/crash-reports/` 에 가짜 crash report 를 남기던 문제. 이제 세 OS 모두 조용히 **종료 코드 0** 으로 끝나며 crash report 를 만들지 않는다(그 외 stdout 오류는 종전대로 에러). 루트 `--help` 가 같은 상황에서 `Error: Broken pipe`(종료 코드 1)를 내던 것도 같은 규칙으로 정리됐다. 근거 ADR-0101.

## [0.10.2] - 2026-08-29

### Added

- `terminal.state`(CLI `tasty terminal state --surface <child>`) — 자식 단건 상태(`idle`/`needs_input`/`active`/`exited`) 조회. `terminal.children`의 항목별 조회와 달리, registry에서 이미 정리된 surface 도 라이브 트리와 대조해 `"exited"`로 구분한다.
- `claude.state`/`codex.state`(CLI `tasty claude state`/`tasty codex state`) — 위 `terminal.state`를 각 plugin 이 자기 namespace 안에서 위임하는 wrapper. `claude.spawn`/`codex.spawn`에 기본 완료 판정 전략(`[[contributes.completion_strategy]] default_for_methods`)이 새로 연결되어, 이 두 메서드에 한해 DAG `poll` 생략 시 spawn 접수를 완료로 오인하던 기존 동작이 뒤집힌다 — 자식이 실제로 idle/exited 가 될 때까지 `running` 을 유지한다.
- `agent.dag_list`/`agent.dag_get`(CLI `tasty agent dag-list`/`dag-get`) — 한 workspace 의 flat 한 task 목록을 **DAG 단위로 쪼갠 조회 표면**. DAG 는 영속 레코드가 아니라 도출된다: `task.metadata.dag` 가 문자열이면 그 값이 그룹 키(explicit, id `d:<값>`), 아니면 `depends_on`/`Fallback.task`/`Reduce.inputs`/`metadata.fallback_of` 4종을 무방향 엣지로 본 약연결 컴포넌트(derived, id `c:<root task id>`). 같은 task 집합이면 id 가 항상 같다. `dag_list` 는 `workspace_id` 를 생략하면 살아있는 전 workspace 를 순회하며(응답 `scope: "live_workspaces"` — 삭제된 workspace 의 고아 task 는 제외), 각 원소에 `name`/`source`/`task_count`/`state_counts`/`rollup_state`/`created_at`/`updated_at`/`root_task_ids`/`has_cycle` 을 싣는다(`include_tasks:true` 면 `task_ids` 도). `dag_get` 은 그 DAG 부분집합만으로 `agent.task_graph` 와 동일한 `nodes`/`edges`(또는 `format:"dot"`)를 낸다. 둘 다 `AgentManage` 권한.
- `agent.task_run`(workspace runner thread start/stop/status)이 이제 plugin 에서도 호출 가능하다(`AgentManage` 권한) — 호스트 재시작 후 runner 는 자동으로 켜지지 않으므로(아래 Changed 참조), plugin 이 자기 workspace 의 runner 를 스스로 되살릴 수단이 필요했다. local-only 로 남는 건 `agent.task_set_result` 와(아래 Changed 참조) `agent.task_await` 뿐이다.
- `agent.task_list`/`agent.task_graph` 응답에 `runner: { running, crashed, ready_count, running_count }` 가 동반된다 — runner 가 꺼져 있어도 `ready_count`/`running_count` 는 store 를 직접 조회한 실제 값이라, "할 일은 있는데 아무도 안 돌리고 있다"가 이 응답만으로 드러난다.
- `agent.task_get` 응답에 `awaiting_external: { wait_key, deadline_ms }` 가 추가됐다 — task 가 push 완료 전략(`AwaitExternal` handle)으로 외부 신호를 기다리는 중일 때만 실려, `state: "running"` 만으로는 구분 안 되던 "그냥 실행 중"과 "외부 보고 대기 중"을 구분할 수 있다.
- `agent.task_create`(CLI `tasty agent task-create`) 응답에 `warnings` 배열이 조건부로 추가됐다 — `on_failure=Fallback` 이면서 `depends_on` 이 비어있지 않은 task 를 생성하면 실린다. 이 조합은 이 task 자신이 실행에서 직접 실패하는 경로(`Running`→`Failed`)에서는 정상 동작하지만, 의존성 실패로 인한 `Waiting`→`Skipped` 전이에는 적용되지 않아(`Fallback` 은 upstream 쪽에 설정해야 하는 필드) 착각하기 쉽다 — task 생성 자체는 막지 않는다.
- `tasty agent task-list`/`task-get`/`task-run` CLI 출력이 raw JSON pretty-print 대신 사람이 터미널에서 바로 읽는 텍스트로 렌더된다(`state  id  name` 목록 + `runner: running (ready=N running=M)` 요약 줄, 정지 상태면 재개 커맨드 안내 포함). 다른 `agent` 서브커맨드(barrier/semaphore/lease/rate-limit/task-graph 등)는 기존과 동일하게 JSON.
- `agent.task_delete`(CLI `tasty agent task-delete`) — task 삭제. 참조(`depends_on`/`Fallback.task`/`Reduce.inputs`) 가 있으면 기본 거부하고 참조자 목록을 `error.data.referenced_by` 에 실어 반환(`-32010`), `--cascade` 는 전이적 참조자까지 함께 삭제, `--force` 는 참조 검사만 우회(상태 제약은 못 뚫음). 삭제 금지 상태는 `running` 하나뿐이며 `--cascade`/`--force` 로도 뚫지 못한다(`-32011`).
- `agent.task_purge`(CLI `tasty agent task-purge`) — 상태 이름(`--states`)·경과시간(`--older-than-ms`) 필터 기반 일괄 삭제. `agent.task_delete` 와 동일한 참조 안전 검사를 적용해, 후보 집합 밖에서 여전히 참조되는 task 는 자동으로 보존한다. `--dry-run` 으로 실제 삭제 없이 계획만 확인 가능.
- 부팅 시 정화 경로(`purge_stale_agent_state_on_boot`)에 자동 GC 가 추가됐다 — 상태 무관 + 7일(잠정) 이상 방치된 task 를 `agent.task_purge` 와 동일한 로직으로 정리한다. memory 자체 TTL(`PutOpts.expires_at`) 은 쓰지 않는다.
- `agent.task_reduce`(CLI `tasty agent task-reduce`)에 `extract_path`(`--extract-path`, RFC 6901 JSON Pointer, 예: `/stdout/text`)가 추가됐다 — 지정하면 reducer 전략 실행 전에 각 input 의 `output` 에서 그 경로만 뽑아낸다. `Run` task 의 `output`(`{pid,stdout:{text,...},stderr:{...}}`)을 구조를 모른 채 `concat_text`/`merge_json` 으로 합성하면 유효한 JSON 도 아니고 뒤 input 이 앞 input 을 통째로 덮어쓰는 문제가 있었는데, 이 옵션으로 leaf 값만 골라 합성할 수 있다. 생략 시 기존 동작(전체 `output`) 유지. 지정된 경로가 없는 input 은 reduce 전체를 실패시키지 않고 `output: null` 로 대체되며, 응답 `warnings` 배열에 사유가 남는다(나머지 input 은 정상 진행).
- `tasty-plugin-sdk`: `HostHandle::self_invoke(method, params)` — plugin 이 자기 자신의 네임스페이스 IPC 메서드를 host 왕복 없이 트리거하는 API. `HostHandle::call` 은 host 가 그 메서드를 self-call 로 forward 하지 않아(trampoline 정책) plugin 자신의 네임스페이스에는 쓸 수 없었는데, 이 메서드는 `&mut plugin` 을 쥔 worker 스레드의 처리 큐에 직접 enqueue해 우회한다. fire-and-forget(host 가 요청한 적 없는 call 이라 응답 대상이 없음) — 실패는 `tracing::warn!` 로만 로그된다.

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

- markdown plugin 의 idle auto-reload(백그라운드 파일 변경 감지)가 항상 `-32601 Method not found: markdown.reload` 로 실패하던 결함 수정 — plugin 이 자기 네임스페이스 IPC 메서드를 `HostHandle::call`(host 왕복)로 부르면, host 의 self-call trampoline 정책(`plugin_ipc.rs`) 때문에 host-native dispatch 로 통과돼 항상 실패했다. `tasty-plugin-sdk` 에 새로 추가된 `HostHandle::self_invoke`(아래 Added)로 우회한다.
- `tasty remote attach --raw`(및 `tasty attach --raw`): 서버/터널 연결이 끊겨도 `--reconnect`(기본 ON) 백오프 재연결이 전혀 동작하지 않던 결함 수정. raw 브리지가 종료 사유와 무관하게 `process::exit(0)` 으로 프로세스를 죽여 재연결 판단 지점(`AttachExit::Disconnected`)에 도달하지 못했다 — 이제 mirror-dump 와 동일하게 채널 기반으로 종료 사유를 구분해 정상 반환한다.
- 완료 판정 전략(`[[contributes.completion_strategy]]`)의 `default_for_methods`/`poll_method` namespace 검증이 plugin owner 를 매니페스트의 reverse-DNS id(예: `com.tasty.claude`)로 비교해, 실제 IPC dispatch 접두어(`claude`)와 절대 일치하지 않아 plugin 소유 전략이 등록 시점에 전부 조용히 drop 되던 결함 수정 — 이제 그 plugin 이 실제로 선언한 `ipc_namespace` 접두어와 비교한다.
- `agent.task_create` 가 `depends_on` 과 달리 `OnFailure::Fallback{task}`/`TaskCommand::Reduce.inputs` 가 가리키는 task id 의 존재를 검증하지 않던 결함 수정. 미존재 fallback 은 main 실패 시 조용히 무시되어 그 main 에 의존하는 downstream 이 영구 `waiting` 에 빠졌고, 미존재 reduce 입력은 dispatch 시점에야 뒤늦게 실패했다. 이제 둘 다 생성 시점에 `-32602` 로 거부된다. 검증 도입 이전에 이미 저장된 dangling 참조는 마이그레이션하지 않는다(신규 생성만 차단) — 그런 참조가 실패 전이를 타면 `tracing::warn!` 을 남긴다.
- `terminal.kill`/`terminal.release`/`terminal.respawn`/`terminal.broadcast`: `--surface`(parent) 생략 시 host 의 단일-parent 폴백이 "라우팅된 그 window 안에서만" 유일성을 봐서, main window 가 2개 이상 열린 세션에서 focused window 로 조용히 새 엉뚱한 window 의 자식 터미널을 조작할 수 있던 결함 수정. 이제 main window 가 2개 이상이면 이 4개 메서드는 `--surface` 없이 호출될 때 명시적 에러로 거부한다(단일 윈도우 세션은 기존처럼 생략 가능 — 하위 호환).

## [0.9.7] - 2026-07-15

많은 변경이 있었음(누적된 릴리스 갭).

## [0.9.6] - 2026-07-15

많은 변경이 있었음(누적된 릴리스 갭).

## [0.9.4] - 2026-07-14

많은 변경이 있었음(누적된 릴리스 갭).
