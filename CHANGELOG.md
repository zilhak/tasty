# Changelog

본 문서는 사용자(AI 에이전트 포함)가 의존하는 표면 — CLI 명령, IPC 메서드, 매니페스트 스키마, plugin 인터페이스 — 의 변경만 기록한다. 내부 refactor·테스트·문서는 `git log`를 참조.

형식: [Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/). 버전: [SemVer](https://semver.org/lang/ko/).

각 변경은 다음 카테고리 중 하나에 속한다:

- `Added` — 새 기능, 새 메서드/명령
- `Changed` — 동작 변경 (BREAK는 머리에 `(BREAK)` 표기)
- `Deprecated` — 폐기 예정, 아직 동작은 함
- `Removed` — 제거된 기능
- `Fixed` — 버그 수정

자세한 안정성 정책·break 분류·deprecation 절차는 [`docs/dev-guide/ipc-stability.md`](docs/dev-guide/ipc-stability.md) 참조.

## [Unreleased]

### Added
- `tasty-agent` 크레이트 — 다중 에이전트 협업 primitive (Phase 5). 신규 namespace `agent.*` + 권한 토큰 `agent` (`Permission::AgentManage`).
  - **Task primitive (Phase 5.1)**: DAG + state 머신. `agent.task_create / task_list / task_get / task_await / task_cancel / task_retry / task_graph` IPC. 영속은 workspace scope `tasty.agent.task.<id>`. TaskState 8종 (`waiting/ready/running/succeeded/failed/cancelled/skipped/unknown`), TaskCommand 4종 (`claude_spawn/run/custom/reduce`), OnFailure 3종 (`abort/continue_downstream/fallback`). `create()` 시 사이클·unknown dependency 검출, state 전이 시 transitive downstream 재평가 (cascade). `task_graph`는 `format=json`(기본) 또는 `format=dot`(Graphviz). 본 sub-phase는 모델/영속/IPC만 책임 — `Ready` 자동 실행 스케줄러, blocking `task_await`, reducer 실행은 후속.
  - CLI: `tasty agent task-{create,list,get,await,cancel,retry,graph}`. `--command`/`--metadata`는 인라인 JSON 또는 `@path/to/file.json`. `--on-failure fallback:<task_id>` 단축 표기 지원
  - **Barrier / Semaphore primitive (Phase 5.2)**: poll-based 동기화 게이트와 자원 점유. `agent.barrier_create / barrier_signal / barrier_await / barrier_state` + `agent.semaphore_create / semaphore_acquire / semaphore_release` IPC. 영속은 workspace scope `tasty.agent.barrier.<name>` / `tasty.agent.semaphore.<name>`. **Barrier**: 상태 `open → closed`(count_required 도달) 또는 `open → timed_out`(timeout 경과); 도장 찍기는 lazy — `signal`/`state`/`list(now_ms)` 호출 시점에 timeout 검사 (별도 스레드 없음). **Semaphore**: N permit 까지 동시 점유; 같은 holder의 재acquire는 idempotent 성공 (retry-safe), permit 회복은 해당 holder의 `release`만. 본 단계는 poll-based — 호출자가 `*_await`를 반복 호출. blocking + queue/wakeup은 scheduler 도입 후.
  - CLI: `tasty agent barrier-{create,signal,await,state}` + `tasty agent semaphore-{create,acquire,release}`
- `tasty-telemetry` 크레이트 — AI 에이전트 관측/비용 계층. 신규 namespace `telemetry.*` (`Telemetry` 권한):
  - **기록**: `telemetry.record { agent?, metric, value, op?∈{set,inc}=set, workspace_id?, tags? }`, `telemetry.record_batch { events: [...] }`. 메트릭 이름은 `^[a-zA-Z][a-zA-Z0-9_]{0,63}$`. 영속 키: `tasty.telemetry.event.{ts:013}.{seq:04}` (workspace_id 있으면 `scope=workspace:<id>`, 없으면 `global`)
  - **조회**: `telemetry.summary { metric?, agent?, workspace_id?, since?, until? }`, `telemetry.timeseries { metric, window∈{1m,5m,1h,1d}, ... }`, `telemetry.top { by?∈{agent,workspace}, limit?=10, ... }` — 모두 raw event prefix scan + 순수 집계
  - **Dispatcher 자동 카운트**: plugin IPC 호출마다 `ipc_calls` (tags `method=<canonical>`) 1 회 자동 기록. agent=`_host` 또는 method=`telemetry.*` 는 측정 생략 (자기측정/재귀 방지)
  - **Cost Cap**: `telemetry.cap.{set,list,remove,status,reset}`. 액션 4종: `notify` (알림만), `stop` / `pause` / `require_approval` (plugin agent 모든 IPC `-32007 cap_blocked` 거부). `require_approval` 은 `approval.request` 도 자동 발행. 차단 해제는 Local caller (CLI) `cap.reset` 만 가능. Stop 의 OS 프로세스 kill 트리거는 `claude.kill` IPC 도입 후 결합 — 현재 Pause 와 실효 동등
  - **이상 탐지**: `CallBurst` 휴리스틱 (1분 1000 회 임계, 1분 dedup 쿨다운). 발화 시 `tasty.telemetry.anomaly.{ts:013}.{id}` Global scope 영속 + 알림. 조회: `telemetry.anomaly.list { agent?, kind?, since?, until? }`. `SlowLoop` / `RssSurge` 는 타입만 정의
  - **세션 요약**: `telemetry.session_summary { workspace_id?, since?, until?, format?∈{markdown,json}=markdown, top_n?=10 }` — tokens (ipc_calls 제외 metric sum) / ipc_calls (total + method top-N) / approvals (total/pending/responded/timed_out/cancelled + choice 분포) / anomalies. LLM 없이 결정론적 집계. workspace_id 미지정 시 전 workspace 합산
  - **Claude Code hook 통합**: `tasty claude hook session-start` 가 시작 시각 기록 → `stop`/`subagent-stop`/`session-end` 에서 `wall_time_ms` 자동 발행. `notification --message <text>` 의 `\btokens?:\s*(\d+)\b` 매칭 시 `input_tokens` 자동 발행 (claude plugin manifest 에 `telemetry` 권한 추가)
  - CLI: `tasty telemetry {record,summary,timeseries,top,session-summary}` + `tasty telemetry cap {set,list,remove,status,reset}` + `tasty telemetry anomaly list`. 가이드: [`docs/agent-guide/telemetry.md`](docs/agent-guide/telemetry.md)
- `tasty-output` 크레이트 — surface 출력 시멘틱 파서 골격. 빌트인 4종(기본 활성): `path` (파일 경로 + line/col), `url` (http/https/ftp/ssh/file), `prompt_boundary` (OSC 133 A/B/C/D), `exit_code` (OSC 133 D 페이로드). 옵션 6종 (명시적 opt-in): `compile_error` (rustc/gcc/clang/tsc, 멀티라인), `stack_trace` (python/rust/node/java, 멀티라인), `test_result` (cargo/pytest/jest), `progress` (bar/size/percent), `osc_link` (OSC 8), `osc_notification` (OSC 9/777). `Parser` trait 에 `parse_block` default impl 신설 — 단일 라인 파서는 변동 없이 `parse_line` 만 구현하면 되고, 멀티라인 파서는 `parse_block` override 로 block 컨텍스트를 본다. 옵저버 스트리밍은 라인 단위 dispatch 만 사용하므로 멀티라인 파서는 `surface.parse_since_mark` (batch) 경로에서만 발화한다. 카탈로그: [`docs/agent-guide/output-parsers.md`](docs/agent-guide/output-parsers.md).
- IPC: `surface.parse_since_mark { surface_id, parsers? }` — read_since_mark 결과를 파서들로 분해해 `items: [{ kind, line, byte_start, byte_end, data }]` 반환. CLI: `tasty read parse-since-mark`.
- OSC 133 명령 인덱싱 — 셸 통합이 보내는 `\e]133;{A|B|C|D};...` 시퀀스를 추적해 surface 별로 `{ prompt_started_at, command_started_at, ended_at, exit_code, command }` JSON 레코드를 `tasty-memory` `scope=surface:<id>` 위에 `tasty.commands.<unix-ms>` 키로 영속화. 새 IPC: `surface.commands { surface_id, limit?, since? }`, `surface.last_command { surface_id }`, `surface.command_at { surface_id, index }` (음수 인덱스 지원, 모두 `TerminalRead` 권한). CLI: `tasty read commands`, `tasty read last-command`, `tasty read command-at --index N`. terminal 엔진에 `TerminalEventKind::PromptBoundary { phase, payload }` 이벤트 신설.
- 출력 옵저버 — PTY 라인 → 빌트인 파서 → sink fan-out 인프라. terminal 엔진에 `TerminalEventKind::OutputAppended { text }` 신설 (Print/PrintString/LineFeed 에서 emit), 호스트에 `ObserverRouter` (per-surface 라인 버퍼 + 파서 dispatch + per-observer bounded channel) + sink worker thread. sink 2 종: `memory` (`scope=global` 위 `tasty.observer.<id>.<ms>` 키로 누적, `max_records` ring buffer), `file` (JSONL append; 기본 경로 `~/.tasty/observers/<id>.jsonl`). Backpressure 정책: bounded channel 가득 차면 새 item drop + `dropped` counter 증가 (PTY freeze 방지). 새 IPC (`TerminalRead`): `output.observe_start { surface_id?, parsers?, kinds?, sink: { type, ... } }`, `output.observe_stop`, `output.observe_list`, `output.observe_info`. CLI: `tasty output observe {start|stop|list|info}`. Surface 가 닫히면 그 surface 에 매인 옵저버 자동 정리 (wildcard `surface_id=None` 옵저버는 유지). socket/fifo sink + 옵저버 spec persistence 는 후속 phase.
- IPC 메서드 별칭 정규화 layer (`src/ipc/alias.rs`). 옛 이름은 호스트가 새 이름으로 자동 매핑하면서 `tracing::warn`을 출력.
- 명명 규칙 자동 검증 테스트 (`src/ipc/method_meta.rs::all_registered_methods_match_naming_policy`).
- Plugin SDK에 `PluginError` 도메인 에러 + `From<PluginError> for IpcMethodError`.
- Plugin surface lifecycle observer — 매니페스트 `[[contributes.surface_observer]] event = "closed"`(`surface.read` 권한 필수)로 구독하면 다른 surface가 닫혔을 때 `Plugin::on_surface_lifecycle(SurfaceLifecycleCtx { event, surface_id, kind, reason })` 콜백을 받는다. fire-and-forget. reason은 `UserClose`(PTY 종료/단축키/탭 우클릭) 또는 `AgentClose`(IPC `surface.close*`). SDK에 `SurfaceLifecycleCtx` / `SurfaceLifecycleEvent` / `SurfaceCloseReason` 노출.
- Plugin manifest 의 예약 IPC prefix 에 `memory`, `output` 추가 — 호스트 메서드와 충돌하지 않도록 plugin 이 해당 namespace 를 점유하면 매니페스트 검증 단계에서 거부된다.
- `tasty-approval` 크레이트 — 휴먼 핸드오프 (요청-응답 결정 게이트). 신규 IPC: `approval.request`, `approval.respond`, `approval.cancel`, `approval.list`, `approval.get`, `approval.history`, `approval.summary.set`, `approval.summary.get` (Permission `Approval`; summary set/get 은 `MemoryWrite`/`MemoryRead` 도 함께 요구). `approval.await` 는 blocking + timeout 이라 plugin 호출 미지원 (`local_only`) — host 내부에서 worker thread 로 수행된다. CLI: `tasty approval {request,respond,await,cancel,get,list,history,summary}`. 영속은 모든 상태 전이를 `tasty.approval.<id>` 키 (workspace_id 가 있으면 `scope=workspace:<id>` 그 외 `global`) 로 기록 — 재시작 후에도 history 조회 가능. severity 별 표시: `info` 알림 only, `warn`/`danger` 는 popup + 알림. popup 은 큐 (`pending_approval_ids`) head 만 그리고, 닫혀도 큐가 남아 있으면 다음 head 로 재오픈. Esc 차단으로 우회 응답 방지.
- 신규 surface kind `diff` — 좌/우 분할로 `before`/`after` 텍스트 (또는 `before_file`/`after_file` 경로) 를 표시한다. 헤더에 Apply/Reject 버튼. Apply 는 `apply_action` 명령을 시스템 클립보드에 복사하고 surface 를 닫는다 (자동 spawn 은 의도적으로 회피 — 사용자 동선 유지). 예: `tasty split --level surface --target this --type diff --meta '{"before":"...","after":"...","apply_action":"git apply /tmp/p.patch"}'`.

### Changed
- `surface.meta_set` / `meta_get` / `meta_unset` / `meta_list` → `surface.meta.set` / `meta.get` / `meta.unset` / `meta.list` (점 표기). 옛 이름은 alias로 동작하지만 deprecated.
- Plugin SDK `HostHandle::call` 반환 타입이 `Result<Value, HostCallError>` → `Result<Value, PluginError>`. `HostCallError`는 `PluginError`의 `#[deprecated]` alias로 유지.
- `surface.meta.*` 가 파일 기반 (`~/.tasty/surfaces/<id>/meta.json` 풍의 임시 디렉터리) 에서 `tasty-memory` 위 `scope=surface:<id>` text/plain entry 로 통합 (응답 형태 동일). 같은 row 가 `memory.*` API 로도 보이며 `memory.changed` 이벤트로 변경이 전파된다. 키 형식 검증 (`[a-z0-9._-]+`, 1..=256) 이 새로 강제되므로 대문자/공백 키는 거부된다.

### Deprecated
- `surface.meta_set` / `surface.meta_get` / `surface.meta_unset` / `surface.meta_list` (underscore 합성). 1.0 tag 직전에 alias 제거.
- `tasty_plugin_sdk::HostCallError` type alias. 새 코드는 `PluginError` 사용.

### Removed
- (없음)

### Fixed
- (없음)
