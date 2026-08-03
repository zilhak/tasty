# 다중 에이전트 협업 (Agent collaboration)

- **Status**: Implemented
- **주체**: AI Agent (여럿이 한 인스턴스 공유)
- **ADR**: 없음
- **코드**: `agent.*` 핸들러(`src/adapters/ipc/handler/agent.rs`), 영속 `tasty-memory`
- **화면**: 없음 (IPC/CLI 전용) — ADR-0040(에이전트 점유는 로컬 사용자에게 시각적으로 구분돼야 한다)·[human-handoff](../human-handoff/index.md) 의 승인 popup 과 달리, task DAG 상태는 *로컬 사용자의 즉각 승인/개입이 필요한 이벤트*가 아니라 에이전트 간 조율 상태다. 관측이 필요한 순간은 그 조율을 요청한 에이전트(또는 그 뒤의 사용자)가 능동적으로 CLI 로 확인하는 시점뿐이라, 상시 노출되는 화면을 두지 않는다.
- **메서드 목록**: [reference/api](../../reference/api.md#에이전트-협업-agent)

## 목적

여러 AI 에이전트가 같은 tasty 인스턴스를 공유할 때 쓰는 협업 primitive 6종. 모두 `agent`(AgentManage) 권한 하나로 grant 된다 — `agent.*` 단일 네임스페이스 + `<verb>_<modifier>` 패턴. 영속은 `tasty.agent.*` memory 키(task/barrier/semaphore/lease 는 `workspace:<id>` scope, rate-limit 은 `global`).

## 내부 동작

### Poll-based 모델

`*_await` 는 blocking 이 아니라 **현재 상태 즉시 응답**(poll). 호출자가 terminal 상태가 아니면 반복 호출한다 — blocking await 의 lock-up 위험을 피하기 위해. (scheduler 도입 시 long-poll 로 분기 예정.)

### 6 primitive

- **Task DAG** — 의존성 그래프 + state 머신. 사이클은 create 시 거부, 상태 변화 시 transitive downstream 자동 재평가. 상태 8종(`waiting/ready/running/succeeded/failed/cancelled/skipped/unknown`). command 4종(`run/custom`(옵션 폴링 `poll` 포함)/`reduce/wait_barrier`). `run` 은 Surface 없는 bare subprocess 로, stdout/stderr 를 각각 마지막 64KiB(tail)까지 캡처해 성공 시 `TaskResult.output`(`{"pid","stdout":{"text","truncated","dropped_bytes"},"stderr":{...}}`)에, 실패 시 에러 메시지에 싣는다(tty 는 미지원 — 필요하면 `pty.*` 사용). OnFailure 3종(`abort`(기본, downstream skip cascade)/`continue_downstream`/`fallback{task}`). `custom` 의 완료 판정은 poll(반복 호출) 뿐 아니라 push(외부 훅 보고)도 지원한다 — 예: `host/command-completed`(OSC 133 셸 통합 기반, 셸 명령 완료를 훅으로 통지받아 exit code 로 성공/실패를 가름). 전략 레지스트리·배선 상세는 [dev-guide/agent-runner](../../dev-guide/agent-runner.md#완료-판정-전략-레지스트리-srccompletion_strategy), `run` 캡처 상세는 [dev-guide/agent-runner](../../dev-guide/agent-runner.md#run-출력-캡처).
  - **참조 검증 범위**: `task_create` 는 `depends_on` 뿐 아니라 `OnFailure::Fallback{task}` 와 `TaskCommand::Reduce.inputs` 가 가리키는 task id 의 존재도 생성 시점에 검증한다(`crates/tasty-agent/src/task/store.rs`) — 미존재면 `-32602` 로 거부. `Fallback{inline}` 은 생성 시점엔 대상이 존재하지 않는 게 정상(실패 전이 시 동적 생성)이라 검증 대상이 아니다. 이 검증은 **신규 생성만** 막는다 — 검증 도입 이전에 이미 저장된 dangling 참조는 마이그레이션하지 않고 그대로 남는다(그 참조를 쓰는 task 는 관측 가능하게 영구 `waiting` 에 머문다; dangling fallback 은 실패 전이 시 `tracing::warn!` 도 남긴다).
  - **`Reduce.inputs` 는 암묵적 의존성**이다 — `depends_on` 과 합쳐 그래프 엣지·사이클 검출 대상이 된다. 다만 readiness 의미는 `depends_on` 과 다르다: reducer(특히 `all`)는 실패한 입력도 의도적으로 수집하므로, 입력 하나가 실패했다고 이 task 를 `skipped` 로 몰지 않는다 — 입력 전부가 *종결*(성공이든 실패든)될 때까지만 `waiting` 을 유지하고, 종결되면 `ready` 로 진행한다.
- **Barrier** — N회 signal 모이면 닫히는 게이트. `timeout_ms` 경과 시 `timed_out` 으로 **lazy 전이**(별도 스레드 없음 — signal/state/list 호출 시 도장).
- **Semaphore** — N permit 동시 점유. 같은 holder 재acquire 는 idempotent(retry-safe), permit 회복은 그 holder 의 release 만.
- **Lease** — 협조적(advisory) 자원 점유 마커 + TTL. OS 락 아님(위반 감지 수준). mode `fail`(충돌 시 `-32009`) / `block`(`acquired:false`). 만료는 list/acquire 시 lazy evict.
- **Reducer** — N task 결과를 단일 값으로 합성. 5전략: `first_success`/`all`/`merge_json`/`concat_text`/`custom`(호스트 shell, stdin 에 결과 배열 JSON). 단발 `agent.task_reduce` 또는 DAG 노드(`TaskCommand::Reduce`, 이 경우 `inputs` 는 암묵적 의존성 — 위 Task DAG 항목 참조).
- **Rate-limit** — (agent, metric) token bucket(보충률 `limit/per_ms`, 상한 `burst`). `global` scope. 누적 임계인 [telemetry cap](../telemetry/index.md) 과 구분(이쪽은 *시간당 비율*). CRUD + `try_consume` 제공 + IPC dispatcher 미들웨어(`should_rate_limit`)가 매 호출 자동 평가.

## 인터페이스

- **AI Agent / CLI**: `tasty agent {task-create,task-list,...,barrier-*,semaphore-*,lease-*,task-reduce,rate-limit-*}`. `--command`/`--metadata` 는 인라인 JSON 또는 `@path`. 전체 표 → [reference/api](../../reference/api.md#에이전트-협업-agent).

## 재시작 동작

task 는 영속되지만(`Scope::Workspace`) runner thread 는 in-memory 다 — 호스트 재시작 후 **runner 는 자동으로 켜지지 않는다.** 대신:

- **부팅 시 정화만 1회** — 라이브 workspace 전부에 대해 stale semaphore/lease holder 회수 + 직전 `Running` task 를 `Failed("host restart")` 로 마감 + persisted `DispatchHandle` reload(살아있는 `ShellProcess`/`PolledDispatch`/`BarrierPoll`/미만료 `AwaitExternal` 는 복원, 죽은 건 마감). runner thread 는 여전히 안 켜져 있으므로 복원된 handle 을 실제로 poll 하려면 수동(또는 plugin) `agent.task_run --action start` 가 필요하다.
- **정지 상태는 조회로 드러난다** — `task_list`/`task_graph` 응답에 `runner: { running, crashed, ready_count, running_count }` 가 동반된다. runner 가 꺼져 있어도 `ready_count`/`running_count` 는 store 의 실제 값이라, "할 일은 있는데 아무도 안 돌리고 있다"가 이 응답만으로 드러난다. `task_get` 은 `AwaitExternal` 로 외부 신호를 기다리는 task 에 `awaiting_external: { wait_key, deadline_ms }` 를 실어 "그냥 running" 과 구분한다.
- **`hook_task_waits`(push 완료 전략의 hook_id → task_id 매핑)는 비영속** — 재시작하면 그 task 는 훅으로는 깨어날 수 없다. 대신 `AwaitExternal` handle 자체가 `deadline_ms` 를 들고 다니므로(핸들은 영속), 다음 재시작의 reload 가 만료 여부를 독자적으로 판정해 마감한다.
- `agent.task_run` 은 plugin 도 호출 가능(`AgentManage`) — 자동 시작이 없으므로 plugin 이 자기 workspace 의 runner 를 스스로 되살릴 수 있어야 하기 때문이다.

상세: [dev-guide/agent-runner](../../dev-guide/agent-runner.md#재시작-계약).

## 에러 코드

`-32004`(not found) · `-32008`(already terminal) · `-32009`(lease conflict) · `-32602`(사이클/미존재 dep(`depends_on`/`Fallback.task`/`Reduce.inputs`)/잘못된 strategy 등) · `-32603`(internal).

## 관련

- [telemetry](../telemetry/index.md) — rate-limit vs cap 구분 · [human-handoff](../human-handoff/index.md) — approval
- [design/systems/memory](../../design/systems/memory.md) — 영속 backing store
- [dev-guide/agent-runner](../../dev-guide/agent-runner.md) — task runner 내부 동작(dispatch/poll, 완료 판정 전략 레지스트리)
