# 협업 primitive (Phase 5)

- **Status**: Implemented

다중 에이전트가 협업할 때 필요한 동기화·의존성·합성 primitive. 신규 단일 namespace `agent`에 Task/Barrier/Semaphore/Lease/Reducer/Rate Limit가 modifier로 묶인다. 신규 권한 토큰 `agent` (`Permission::AgentManage`).

### Task primitive (Phase 5.1)

**DAG + state 머신** — `tasty-agent` 크레이트에 `Task`/`TaskState`/`TaskCommand`/`TaskGraph`/`TaskStore` 정의. 영속은 `tasty.agent.task.<id>` 키, scope = `workspace:<id>`.

- **TaskState 8종**: `Waiting` (의존성 미충족) / `Ready` / `Running` / `Succeeded` / `Failed { error }` / `Cancelled` / `Skipped` (의존성 실패로 자동 스킵) / `Unknown` (재시작 후 Running이던 task — 사용자가 retry/cancel 결정 필요)
- **TaskCommand 4종**: `ClaudeSpawn` (claude.spawn 호출) / `Run` (terminal에서 명령 실행) / `Custom` (임의 IPC 위임) / `Reduce` (5종 reducer 전략) — 실제 실행 wiring은 후속 sub-phase
- **OnFailure 3종**: `Abort` (downstream 모두 Skipped, 기본) / `ContinueDownstream` (실패를 성공처럼 취급) / `Fallback { task?, inline? }` (대체 task로 우회 — Phase 5.6 부터 main 실패 시 fallback task 가 자동 Ready, fallback 의 succeed/fail 도 main 의 downstream 으로 전파됨. Phase J.A 부터 `inline: InlineFallbackSpec` 으로 동적 생성 지원 — main Failed 시 `TaskStore::create` 가 새 task 발급, `metadata.fallback_of` 로 idempotency)
- **사이클 검출**: DFS 3-color로 `create()` 시점에 검증. unknown dependency도 같은 단계에서 거부
- **자동 cascade**: 임의 task의 state가 바뀌면 transitive downstream을 재평가해 `Waiting → Ready/Skipped`로 자동 전이

### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.task_create` | AgentManage | 새 task 생성. `workspace_id`/`name`/`command`/`depends_on?`/`on_failure?`/`metadata?` |
| `agent.task_list` | AgentManage | 워크스페이스 task 목록. `state?` 필터 |
| `agent.task_get` | AgentManage | 단건 조회 |
| `agent.task_await` | AgentManage | task 가 종결 상태에 도달할 때까지 **blocking** 대기 (Phase J.A). 응답 `{outcome: "terminal"|"timed_out"|"not_found", state?, result?}`. `timeout_ms` 미지정 또는 0 = 무한 대기. TaskWakerHub 가 set_state 종결 분기에서 fire |
| `agent.task_cancel` | AgentManage | 명시적 취소. downstream cascade |
| `agent.task_retry` | AgentManage | Failed/Cancelled/Skipped/Unknown task 재시작. `reset_downstream?`로 downstream도 Waiting으로 |
| `agent.task_graph` | AgentManage | DAG 출력. `format`=`json` (기본) 또는 `dot` (Graphviz) |
| `agent.task_run` | AgentManage | Workspace runner thread 시작/중단/상태. `action` ∈ {start, stop, status}. 응답: `{running, crashed, ready_count, running_count}`. Phase H.F |
| `agent.task_set_result` | AgentManage | 외부/수동 task 의 terminal 결과 보고. `state` ∈ {succeeded, failed}, `output?`/`error?`/`exit_code?`. Phase H.F |

### CLI

```
tasty agent task-create --workspace-id <id> --name <n> --command @spec.json [--depends-on T1,T2] [--on-failure abort|continue_downstream|fallback:T3] [--metadata @meta.json]
tasty agent task-list   --workspace-id <id> [--state <s>]
tasty agent task-get    --workspace-id <id> --id <T>
tasty agent task-await  --workspace-id <id> --id <T> [--timeout-ms <ms>]
tasty agent task-cancel --workspace-id <id> --id <T>
tasty agent task-retry  --workspace-id <id> --id <T> [--reset-downstream]
tasty agent task-graph  --workspace-id <id> [--format json|dot]
tasty agent task-run    --workspace-id <id> [--action start|stop|status]
tasty agent task-set-result --workspace-id <id> --id <T> --state succeeded|failed [--output @out.json] [--error <msg>] [--exit-code <n>]
```

`--command`/`--metadata`는 인라인 JSON 또는 `@path` (파일 로드). `--on-failure fallback:<task_id>`처럼 `kind`만 단축 표기.

본 sub-phase는 **state 머신 + 영속 + IPC/CLI 표면**만 책임진다. `Ready` task를 실제로 실행하는 스케줄러, blocking `task_await`, reducer 실행, lease/rate-limit는 후속 sub-phase에서 추가된다.

### Task runner (Phase H.F)

**executor 루프** — `Ready` task 를 자동 dispatch 하고 `Running` task 의 완료를 polling 으로 감지해 state 를 진행시키는 host 측 thread. workspace 1개당 1개, 500ms tick. 상세: [dev-guide/agent-runner.md](dev-guide/agent-runner.md).

- `TaskExecutor` trait (`tasty-agent::runner`) — pure 로직. `dispatch` (비차단) + `poll` (1tick) 두 메서드. host 가 `HostExecutor` 로 구현.
- `HostExecutor` (`src/core/agent/runner_host.rs`) — `ClaudeSpawn` → `claude.spawn` 동기 IPC + `claude.wait` polling. `Run` → `std::process::Command` + `try_wait`. `Custom` → host IPC 동기 dispatch. `Reduce` → 즉시 collect + `reduce_with_custom`.
- `RunnerRegistry` (`src/core/agent/runner_thread.rs`) — workspace 별 thread 의 start/stop/status. 중복 start no-op (idempotent). panic 시 `catch_unwind` 흡수 + crashed 마킹.
- `HostIpcInjector` (`src/app/ipc/host_call.rs`) — off-main thread 가 plugin IPC 메서드를 동기 호출하는 통로. `IpcCommand` 를 App 큐에 직접 push + `IpcWaker` 깨움 + `sync_channel(1)` `recv_timeout(5s)`.
- `crates/tasty-agent/src/platform/process_alive.rs` — cross-platform pid liveness probe (Unix `kill(pid, 0)` / Windows `OpenProcess + GetExitCodeProcess`).

Phase H.F 시점에는 handle 이 runner thread 메모리에만 — Phase J.A 에서 영속 + restart reload 로 진화.

**Phase J.A — runner 완성**:
- **DispatchHandle 영속** (`tasty.agent.handle.<task_id>`): Started 직후 영속,
  release_permit 시 evict. 호스트 재시작 시 `reload_persistent_handles` 가
  pid liveness 검사 (`process_alive::is_alive`) — alive 복원 / dead Failed 마감.
- **Lease-gated dispatch**: `task.metadata.lease = {resource, holder?, ttl_ms?, mode?}`
  컨벤션. dispatch 게이트 순서 lease → semaphore (R-4 dead-lock 회피).
- **`OnFailure::Fallback { inline }` 동적 task 생성**: main Failed 시
  `InlineFallbackSpec` 으로 새 task 발급. `metadata.fallback_of` 로 idempotency.
- **`agent.task_await` 진짜 blocking**: `TaskWakerHub` (sync_channel + waiters
  HashMap + recv_timeout). set_state 종결 분기 / Core wrapper / runner thread tick
  의 모든 경로에서 fire.

**Phase K.A — runner 잔여 한계 fix**:
- **`ShellProcess` exit_code 정확 복원**: `HostExecutor::dispatch` 가 Run 자식 spawn
  직후 watcher thread 를 띄워 `child.wait()` 종료 status 를
  `tasty.agent.run_result.<task_id>` 영속 + shared cell 양쪽에 기록. poll 은 cell
  만 조회 (try_wait 우회), reload 의 dead pid 분기는 영속을 조회해 exit_code 까지
  정확히 Succeeded / Failed 마감 (`precise` 분기). host 가 spawn 과 watcher 의
  영속 완료 사이에 죽으면 손실 — cross-platform 으로 회피 불가.
- **`ClaudeChild` reload injector grace**: poll 첫 dispatch 실패가 injector 미초기화
  사유면 deadline = now + `INJECTOR_GRACE_MS (30s)` 세팅 → 도래 전까지는 `Active`
  로 흡수, 도래 후이면 `Failed("injector grace expired")`. injector 외 Err (timeout
  등) 는 기존대로 즉시 Failed. 정상 dispatch 1회 성공 시 deadline reset.

**Semaphore-gated dispatch + WaitBarrier task (Phase I.A)** — `TaskExecutor::dispatch` 가 `DispatchOutcome::{Started, Deferred, PermanentFail}` 3-way 결과를 반환. `task.metadata.semaphore = { name, holder? }` 컨벤션이 있으면 dispatch 진입에서 `SemaphoreStore::acquire` 시도, 부족 시 `Deferred` 로 다음 tick 재시도. permit 회수는 종결 (Succeeded/Failed/Cancelled) 시 자동. 추가로 `TaskCommand::WaitBarrier { name }` 로 DAG 안에서 명시적 barrier gate 가능 — barrier `Closed` → Succeeded, `TimedOut` → Failed. 호스트 재시작 시 영속된 holder 의 leak 방지를 위해 workspace runner thread 시작 직전 `holder == task.id` 컨벤션이 맞는 Running 잔여 task 의 permit 정화 + Failed("host restart") 마감 단계 1 회. `metadata.semaphore.holder` 가 다르면 *외부 도구가 직접 acquire 한 항목* 으로 간주, 정화 대상 아님. 상세: [dev-guide/agent-runner-primitives.md](dev-guide/agent-runner-primitives.md).

### Barrier / Semaphore primitive (Phase 5.2)

**poll-based 동기화 게이트와 자원 점유** — `tasty-agent` 크레이트에 `Barrier`/`BarrierState`/`BarrierStore`, `Semaphore`/`AcquireOutcome`/`ReleaseOutcome`/`SemaphoreStore` 정의. 영속 키는 각각 `tasty.agent.barrier.<name>` / `tasty.agent.semaphore.<name>`, scope = `workspace:<id>`.

- **Barrier**: N개 신호가 모일 때까지 기다리는 게이트. 상태 `Open → Closed` (count 충족) 또는 `Open → TimedOut` (timeout 경과). 도장 찍기는 lazy — `signal` / `state` / `list(now_ms)` 호출 시점에 timeout 검사. 별도 스레드/타이머 없음.
- **Semaphore**: N permit 까지 동시 점유 허용. 같은 holder의 재acquire는 idempotent 성공 (retry-safe). `acquired:false` 응답으로 polling, permit 회복은 `release` (지정 holder가 점유 중일 때만).

이 단계도 **poll-based**다. 호출자가 `*_await`를 반복 호출하며 polling 한다. blocking + queue/wakeup은 scheduler 도입 후 추가.

#### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.barrier_create` | AgentManage | `workspace_id`/`name`/`count_required≥1`/`timeout_ms?`. 이름 중복은 `-32602` |
| `agent.barrier_signal` | AgentManage | count_signaled++. 도달 시 `Closed`, timeout 경과 시 `TimedOut` + 거부 |
| `agent.barrier_await` | AgentManage | 현 단계: `barrier_state`와 동일 (즉시 응답) |
| `agent.barrier_state` | AgentManage | 현 상태 (조회 시점에 timeout 도장 적용) |
| `agent.barrier_list` | AgentManage | `{ total, barriers: [...] }`. 조회 시점에 timeout 도장 적용 |
| `agent.barrier_delete` | AgentManage | barrier 삭제. 존재하지 않으면 no-op |
| `agent.semaphore_create` | AgentManage | `workspace_id`/`name`/`permits≥1` |
| `agent.semaphore_acquire` | AgentManage | `{ acquired, semaphore }`. 동일 holder는 idempotent |
| `agent.semaphore_release` | AgentManage | `{ released, semaphore }`. 점유 중이 아니면 no-op |
| `agent.semaphore_list` | AgentManage | `{ total, semaphores: [...] }` |
| `agent.semaphore_delete` | AgentManage | semaphore 삭제. 존재하지 않으면 no-op |

#### CLI

```
tasty agent barrier-create   --workspace-id <id> --name <n> --count-required <N> [--timeout-ms <ms>]
tasty agent barrier-signal   --workspace-id <id> --name <n>
tasty agent barrier-await    --workspace-id <id> --name <n>
tasty agent barrier-state    --workspace-id <id> --name <n>
tasty agent barrier-list     --workspace-id <id>
tasty agent barrier-delete   --workspace-id <id> --name <n>
tasty agent semaphore-create --workspace-id <id> --name <n> --permits <N>
tasty agent semaphore-acquire --workspace-id <id> --name <n> --holder <h>
tasty agent semaphore-release --workspace-id <id> --name <n> --holder <h>
tasty agent semaphore-list   --workspace-id <id>
tasty agent semaphore-delete --workspace-id <id> --name <n>
```

### Lease primitive (Phase 5.3)

**협조적(advisory) 자원 점유 + TTL** — `tasty-agent::lease`. 다중 에이전트가 임의 resource(예: `file:/path`, `workspace:foo`)의 점유 상태를 공유하기 위한 마커. OS 락이 아니므로 lease 를 무시한 채 resource 를 만지는 행위 자체는 막지 못한다. 영속 키는 `tasty.agent.lease.<encoded-resource>`, scope = `workspace:<id>`. resource 문자열은 memory 키 허용 문자(`[a-z0-9._-]`)로 escape 되어 저장 (디코딩 불필요 — 원본은 JSON 에 같이 저장).

- **상태**: `{ workspace_id, resource, holder, acquired_at, expires_at? }`. `ttl_ms` 가 있으면 `expires_at = acquired_at + ttl_ms`. 만료 lease 는 다음 `acquire` 또는 `list` 호출 시점에 lazy 하게 evict
- **모드**: `fail` (기본 — 충돌 시 `-32009 lease_conflict` 즉시 실패) / `block` (충돌 시 `acquired:false` 반환, 호출자가 polling)
- **점유 규칙**: 같은 holder 재acquire 는 idempotent 갱신 (TTL 재설정). release 는 점유 holder 만 가능 — 다른 holder 호출은 no-op
- **한계**: 협조적 마커이므로 lease 를 보지 않는 외부 프로세스의 접근은 차단되지 않는다. 진정한 락이 필요하면 OS flock/fcntl 을 별도로 써야 한다

#### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.lease_acquire` | AgentManage | `workspace_id`/`resource`/`holder`/`ttl_ms?`/`mode?`. 충돌 + `fail` 시 `-32009 lease_conflict`, `block` 시 `acquired:false` |
| `agent.lease_release` | AgentManage | `{ released, lease? }`. 점유 중이 아니면 no-op |
| `agent.lease_list` | AgentManage | `{ total, leases: [...] }`. 만료 lease 자동 evict |

#### CLI

```
tasty agent lease-acquire --workspace-id <id> --resource <r> --holder <h> [--ttl-ms <ms>] [--mode fail|block]
tasty agent lease-release --workspace-id <id> --resource <r> --holder <h>
tasty agent lease-list    --workspace-id <id>
```

### Reducer (Phase 5.4)

**N개 task 의 결과를 단일 값으로 합성** — `tasty-agent::reducer` 모듈에 4종 in-process 전략 + 1종 host-bridged 전략(`custom`). 본 단계는 동기적으로 동작 — 입력 task 가 아직 끝나지 않았으면 `output` 은 `null` 로 들어간다 (완료 보장은 호출자 책임).

| 전략 | 동작 |
|---|---|
| `first_success` | 첫 `Succeeded` task 의 `output`. 성공한 입력이 없으면 `-32602` |
| `all` | 모든 입력 `output` 을 순서대로 JSON 배열로 (상태 무관) |
| `merge_json` | 모든 입력 `output` (JSON object) 을 left-to-right deep merge. non-object 는 거부 |
| `concat_text` | 모든 입력 `output` 을 텍스트로 이어 붙임 |
| `custom` | 호스트 shell (`sh -c` / `cmd /C`) 로 명령 실행, stdin 에 `[output1, output2, ...]` JSON 배열, stdout 이 결과 (JSON 시도 → 실패 시 string) |

#### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.task_reduce` | AgentManage | `workspace_id`/`inputs: [TaskId]`/`strategy: { kind, command? }` → `{ value }`. 입력 task 부재는 `-32004` |

#### CLI

```
tasty agent task-reduce --workspace-id <id> --inputs T1,T2,T3 --strategy first_success|all|merge_json|concat_text|custom:<command>
```

### Rate limit (Phase 5.5)

**시간당 비율 제한 (token bucket)** — `tasty-agent::rate_limit`. (agent, metric) 쌍에 대해 `limit` 토큰 / `per_ms` 윈도우의 보충률로 차감-기반 제한을 건다. 영속 키는 `tasty.agent.rate_limit.<id>`, scope = `Global` (워크스페이스 무관 — agent 전역 비율).

**`telemetry.cap` (04) 과의 차이:**

| 시스템 | 의미 | 차단 시점 |
|---|---|---|
| `telemetry.cap` | 누적 임계 (예: `input_tokens` 총합 ≥ 100000) | 합산값이 임계 도달 시 |
| `agent.rate_limit` | 시간당 비율 (예: `ipc_calls` 100/분) | 윈도우 내 토큰 소진 시 |

`burst` 가 비면 `burst = limit` 으로 기본값을 채워 burst-허용 없이 즉시 윈도우 동등 차감으로 동작. 등록되지 않은 (agent, metric) 쌍에 대한 `try_consume` 은 항상 허용 (rate_limit 미적용 = throttle 안 함).

**IPC dispatcher 미들웨어 (Phase I.A)** — 모든 비-Local / 비-`_host` IPC 호출은 dispatcher 진입에서 (agent, `ipc_calls`) 1 차감을 자동 시도한다. throttle 차단 시 `-32010 throttled` 응답 + audit Deny. 면제: `telemetry.*` (재귀 폭주), `agent.rate_limit_*` (자가 회복 경로 — 차단 시 throttle 된 agent 가 영구 차단됨), `system.info`. 정책상 throttled 분기는 `record_ipc_call` 을 건너뛰므로 `ipc_calls` 텔레메트리 이벤트로 카운트되지 않는다. throttle 폭주 추적은 `RateLimit.throttled_count` 가 담당.

#### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.rate_limit_set` | AgentManage | `agent`/`metric`/`limit`/`per_ms`/`burst?` upsert. 동일 (agent, metric) 은 같은 id 유지하며 버킷 reset |
| `agent.rate_limit_list` | AgentManage | 전체 버킷 (refill 적용 후) `{ total, rate_limits: [...] }` |
| `agent.rate_limit_remove` | AgentManage | `id` 로 삭제 |
| `agent.rate_limit_status` | AgentManage | `agent?` / `metric?` 필터로 현재 상태 조회 |

#### CLI

```
tasty agent rate-limit-set    --agent <id> --metric <name> --limit <n> --per-ms <ms> [--burst <n>]
tasty agent rate-limit-list
tasty agent rate-limit-remove --id <rate-limit-id>
tasty agent rate-limit-status [--agent <id>] [--metric <name>]
```

### 실패 처리 / Retry (Phase 5.6)

**3 가지 OnFailure 정책 + retry 의 downstream 리셋 옵션**:

| 정책 | 메인 task 실패 시 동작 |
|---|---|
| `Abort` (기본) | downstream 전부 `Skipped` 로 cascade |
| `ContinueDownstream` | 실패를 성공처럼 취급 — downstream 은 정상 `Ready` |
| `Fallback { task }` | main 실패 시 fallback task 가 자동 `Ready`. fallback 이 `Succeed` 하면 main 의 downstream 도 `Ready`, fallback 도 실패하면 downstream `Skipped`. main 의 downstream 은 fallback 결과 나올 때까지 `Waiting` 유지 |

`agent.task_retry { id, reset_downstream? }` — `Failed` / `Cancelled` / `Skipped` / `Unknown` 상태의 task 를 `Waiting` 으로 되돌려 dep 평가로 자동 재진행. `reset_downstream=true` 면 transitive downstream 중 `Skipped` / `Failed` / `Cancelled` 도 `Waiting` 으로 되돌려 한 번에 재시도 가능.
