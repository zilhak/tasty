# 다중 에이전트 협업 (Agent collaboration)

- **Status**: Implemented
- **주체**: AI Agent (여럿이 한 인스턴스 공유)
- **ADR**: [ADR-0066](../../adr/0066-task-graph-view-deferred.md)(task-graph 화면은 순서상 유보 — 영구 배제 아님)
- **코드**: `agent.*` 핸들러(`src/adapters/ipc/handler/agent.rs`), 영속 `tasty-memory`
- **화면**: 없음 (IPC/CLI 전용) — 근거·재검토 조건은 [ADR-0066](../../adr/0066-task-graph-view-deferred.md) 참조.
- **메서드 목록**: [reference/api](../../reference/api.md#에이전트-협업-agent)

## 목적

여러 AI 에이전트가 같은 tasty 인스턴스를 공유할 때 쓰는 협업 primitive 6종. 모두 `agent`(AgentManage) 권한 하나로 grant 된다 — `agent.*` 단일 네임스페이스 + `<verb>_<modifier>` 패턴. 영속은 `tasty.agent.*` memory 키(task/barrier/semaphore/lease 는 `workspace:<id>` scope, rate-limit 은 `global`).

## 내부 동작

### `*_await` — 메서드마다 다르다

`barrier_await` 는 blocking 이 아니라 **현재 상태 즉시 응답**(poll, `barrier_state` 의 alias) — 호출자가 terminal 상태가 아니면 반복 호출한다.

`task_await` 는 **진짜 blocking** 이다(`TaskWakerHub` 기반, 워커 스레드에서 처리). `timeout_ms` 생략 시 기본 10분(600,000ms, 잠정값 — 실사용 경험이 쌓이면 재조정)까지 대기하고, 그 안에 terminal 에 도달하지 못하면 `{"outcome":"timed_out"}` 으로 반환한다. `timeout_ms: 0` 을 명시하면 이 기본값을 우회해 무한 대기한다. **local caller 전용**(`local_only`) — `approval.await` 와 대칭으로, plugin SDK 는 단일 워커 스레드가 요청을 직렬 처리하므로 plugin 이 `task_await` 로 블록되면 자기 자신의 다른 host→plugin 요청을 전혀 처리하지 못한다(자기 자신이 호출한 응답 수신은 별도 경로라 자기-교착까지는 아니지만, 그 task 가 자신을 다시 호출하는 구성이면 dispatch 타임아웃(5s)으로 정상 작업이 실패한다). plugin 은 대신 완료 판정 전략(`[[contributes.completion_strategy]]`)을 선언해 러너가 대신 기다리게 하거나, `task_get` 을 폴링한다.

### 6 primitive

- **Task DAG** — 의존성 그래프 + state 머신. 사이클은 create 시 거부, 상태 변화 시 transitive downstream 자동 재평가. 상태 8종(`waiting/ready/running/succeeded/failed/cancelled/skipped/unknown`). command 4종(`run/custom`(옵션 폴링 `poll` 포함)/`reduce/wait_barrier`). `run` 은 Surface 없는 bare subprocess 로, stdout/stderr 를 각각 마지막 64KiB(tail)까지 캡처해 성공 시 `TaskResult.output`(`{"pid","stdout":{"text","truncated","dropped_bytes"},"stderr":{...}}`)에, 실패 시 에러 메시지에 싣는다(tty 는 미지원 — 필요하면 `pty.*` 사용). OnFailure 3종(`abort`(기본, downstream skip cascade)/`continue_downstream`/`fallback{task}`). `custom` 의 완료 판정은 poll(반복 호출) 뿐 아니라 push(외부 훅 보고)도 지원한다 — 예: `host/command-completed`(OSC 133 셸 통합 기반, 셸 명령 완료를 훅으로 통지받아 exit code 로 성공/실패를 가름). 전략 레지스트리·배선 상세는 [dev-guide/agent-runner](../../dev-guide/agent-runner.md#완료-판정-전략-레지스트리-srccompletion_strategy), `run` 캡처 상세는 [dev-guide/agent-runner](../../dev-guide/agent-runner.md#run-출력-캡처).
  - **참조 검증 범위**: `task_create` 는 `depends_on` 뿐 아니라 `OnFailure::Fallback{task}` 와 `TaskCommand::Reduce.inputs` 가 가리키는 task id 의 존재도 생성 시점에 검증한다(`crates/tasty-agent/src/task/store.rs`) — 미존재면 `-32602` 로 거부. `Fallback{inline}` 은 생성 시점엔 대상이 존재하지 않는 게 정상(실패 전이 시 동적 생성)이라 검증 대상이 아니다. 이 검증은 **신규 생성만** 막는다 — 검증 도입 이전에 이미 저장된 dangling 참조는 마이그레이션하지 않고 그대로 남는다(그 참조를 쓰는 task 는 관측 가능하게 영구 `waiting` 에 머문다; dangling fallback 은 실패 전이 시 `tracing::warn!` 도 남긴다).
  - **`Reduce.inputs` 는 암묵적 의존성**이다 — `depends_on` 과 합쳐 그래프 엣지·사이클 검출 대상이 된다. 다만 readiness 의미는 `depends_on` 과 다르다: reducer(특히 `all`)는 실패한 입력도 의도적으로 수집하므로, 입력 하나가 실패했다고 이 task 를 `skipped` 로 몰지 않는다 — 입력 전부가 *종결*(성공이든 실패든)될 때까지만 `waiting` 을 유지하고, 종결되면 `ready` 로 진행한다.
  - **삭제(`task_delete`)/일괄 삭제(`task_purge`)**: 참조(`depends_on` ∪ `Fallback.task` ∪ `Reduce.inputs`)가 있는 task 삭제는 기본 거부하고 참조자 목록을 `error.data.referenced_by` 에 실어 반환(`-32010`) — dangling 참조로 인한 `create()` 실패(`UnknownDependency`)·downstream 영구 `waiting` 을 막기 위함. `cascade:true` 는 전이적 참조자 전부를 함께 지우고, `force:true` 는 참조 검사만 우회한다(dangling 참조는 호출자 책임). 삭제 금지 상태는 `running` 하나뿐이다(`-32011`) — `waiting`/`ready`/종결 상태는 `cascade`/`force` 여부와 무관하게 항상 허용된다. 이 제약이 terminal 로 좁지 않고 `running` 하나뿐인 이유: 방치된 `waiting` task(예: 입력이 끝나지 않는 `Reduce`)를 terminal-only 제약으로는 영원히 못 지우고, 그게 참조로 자기 입력들을 붙잡아 그 입력들도 영영 GC 대상에서 빠지기 때문. `task_purge` 는 상태 이름 목록(`states`)·경과시간(`older_than_ms`) 필터로 후보를 고르되, 후보 집합 밖에서 참조되는 task 는 자동으로 보존(`retained`)한다 — `dry_run:true` 로 실제 삭제 없이 계획만 확인할 수 있다. 삭제가 실제로 이뤄지면 `tasty.agent.handle.<id>`/`tasty.agent.run_result.<id>` side-key 도 함께 정리된다.
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
- **같은 부팅 정화 경로에서 자동 GC 도 함께 돈다** — 상태 무관 + 잠정 임계값(7일) 이상 방치된 task 를 `task_purge` 와 동일한 참조 안전 로직(`plan_sweep`/`apply_sweep_plan`)으로 쓸어낸다. memory 자체 TTL(`PutOpts.expires_at`)은 쓰지 않는다 — TTL 만료는 참조 무결성·상태 검사를 우회해 dangling 참조를 재도입하기 때문. 상세: [dev-guide/agent-runner](../../dev-guide/agent-runner.md#자동-gc).
- **정지 상태는 조회로 드러난다** — `task_list`/`task_graph` 응답에 `runner: { running, crashed, ready_count, running_count }` 가 동반된다. runner 가 꺼져 있어도 `ready_count`/`running_count` 는 store 의 실제 값이라, "할 일은 있는데 아무도 안 돌리고 있다"가 이 응답만으로 드러난다. `task_get` 은 `AwaitExternal` 로 외부 신호를 기다리는 task 에 `awaiting_external: { wait_key, deadline_ms }` 를 실어 "그냥 running" 과 구분한다.
- **`hook_task_waits`(push 완료 전략의 hook_id → task_id 매핑)는 비영속** — 재시작하면 그 task 는 훅으로는 깨어날 수 없다. 대신 `AwaitExternal` handle 자체가 `deadline_ms` 를 들고 다니므로(핸들은 영속), 다음 재시작의 reload 가 만료 여부를 독자적으로 판정해 마감한다.
- `agent.task_run` 은 plugin 도 호출 가능(`AgentManage`) — 자동 시작이 없으므로 plugin 이 자기 workspace 의 runner 를 스스로 되살릴 수 있어야 하기 때문이다.

상세: [dev-guide/agent-runner](../../dev-guide/agent-runner.md#재시작-계약).

## 에러 코드

`-32004`(not found) · `-32008`(already terminal) · `-32009`(lease conflict) · `-32010`(task 참조 중 — `task_delete` 기본 거부, `error.data.referenced_by` 에 참조자 목록) · `-32011`(task 가 `running` — 삭제 불가, `cancel` 선행 필요) · `-32602`(사이클/미존재 dep(`depends_on`/`Fallback.task`/`Reduce.inputs`)/잘못된 strategy 등) · `-32603`(internal).

## 관련

- [telemetry](../telemetry/index.md) — rate-limit vs cap 구분 · [human-handoff](../human-handoff/index.md) — approval
- [design/systems/memory](../../design/systems/memory.md) — 영속 backing store
- [dev-guide/agent-runner](../../dev-guide/agent-runner.md) — task runner 내부 동작(dispatch/poll, 완료 판정 전략 레지스트리)
- [ADR-0066](../../adr/0066-task-graph-view-deferred.md) — task-graph 화면 부재의 근거·재검토 조건
