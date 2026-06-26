# Agent task runner

workspace 단위 thread 1개가 `Ready` task 를 자동 dispatch 하고 `Running` task 완료를 polling 으로 감지해 state 머신을 진행시키는 *task DAG executor*. 상태 머신·영속은 [`tasty-agent`](../../crates/tasty-agent/) 가, 실제 *실행* 만 host 가 위임받는다. IPC/CLI 표면 명세는 [reference/api](../reference/api.md)("agent" namespace).

## 구성

| 파일 | 역할 |
|------|------|
| `crates/tasty-agent/src/runner.rs` | `TaskExecutor` trait + `RunnerLoop::tick`(순수 로직) |
| `crates/tasty-agent/src/platform/` | cross-platform pid liveness probe(`process_alive`) |
| `src/core/agent/runner_host.rs` | `HostExecutor` — `TaskExecutor` host 구현 + `RunnerContext`(memory + agent_seq + host_ipc injector) |
| `src/core/agent/runner_thread.rs` | `RunnerRegistry` — workspace 별 thread start/stop/status + 재시작 정화 |
| `src/app/ipc/host_call.rs` | `HostIpcInjector` — runner thread 가 plugin IPC 를 동기 호출하는 통로 |
| `src/adapters/ipc/handler/agent/` | `task`/`barrier`/`semaphore`/`lease`/`ratelimit` IPC 핸들러 |

## 모델

`TaskExecutor` 는 `dispatch`(비차단 실행 시작 → 핸들) + `poll`(1 tick 현재 상태) 두 메서드. 호스트는 polling interval 마다 `RunnerLoop::tick` 호출:

```text
1. Running task → executor.poll(handle)
   Active → 유지 / Done(result) → Succeeded / Failed(err) → Failed
2. Ready task → executor.dispatch(task)  → DispatchOutcome 3-way:
   Started(h)    → handle 보관 + Ready→Running
   Deferred      → 이번 tick 불가(permit 부족 등). state 전이 X, 다음 tick 재평가
   PermanentFail(e) → ImmediateFail handle wrap → 다음 tick poll 에서 Failed 흡수
```

state 전이는 `tasty-agent` 의 `is_valid_transition` 표를 따른다. `Ready→Failed` 직접 전이는 불허라, dispatch 실패도 *먼저 Running 으로* 보낸 뒤 다음 tick 에서 Failed 로 흡수한다.

### DispatchHandle

`PolledDispatch { workspace_id, poll_method, poll_params, state_field, terminal_states, interval_ms, deadline_ms }`(범용 폴링 — dispatch 시점에 완성된 `poll_params` 로 terminal 상태 도달까지 `poll_method` 반복 호출) · `ShellProcess { pid }`(`Run` task 자식; `Child` 객체는 Clone 불가라 executor 의 `shell_children` map 에 별도 보관) · `BarrierPoll { workspace_id, name }` · `ReduceImmediate`/`CustomImmediate`/`ImmediateFail`(dispatch 시점 즉시 결정).

### HostExecutor 매핑

| TaskCommand | dispatch | poll |
|-------------|----------|------|
| `Run { command, cwd }` | `Command::spawn` → pid → `ShellProcess`. 빈 command Err | watcher thread 의 `child.wait()` 결과 cell 조회 → exit 0 Done / 아니면 Failed |
| `Custom { ipc_method, params, poll: None }` | host IPC dispatch(timeout 5s) → `CustomImmediate` | 즉시 Done |
| `Custom { ipc_method, params, poll: Some(spec) }` | host IPC dispatch → `map_from_request`/`map_from_response` 로 `poll_params` 완성 → `PolledDispatch` | `poll_method` 호출 → `state_field` 가 `terminal_states` 중 하나면 Done(응답 전체가 산출물) / 아니면 Active(`deadline_ms` 초과 시 Failed) |
| `Reduce { inputs, strategy }` | input 결과 collect → `reduce_with_custom` → `ReduceImmediate` | 즉시 Done |
| `WaitBarrier { name }` | `BarrierPoll` | `Open`→Active / `Closed`→Done / `TimedOut`→Failed |

> 자식 에이전트(예: `claude.spawn` + `claude.wait`)는 이 범용 `Custom { poll }` 메커니즘의 한 사용자다 — 코어는 특정 에이전트를 모른 채 임의 IPC dispatch→폴링을 표현한다. CLI auto_wait(`AutoWaitDecl`/`PollingDecl`)와 동형 스펙으로 폴링 semantics 를 통일한다.

## RunnerRegistry

`Core::agent_runner_registry()` 로 접근. workspace 1개당 thread 1개:

- `start(ctx, ws) -> bool` — 이미 실행 중이면 false(idempotent). crashed 면 정리 후 재시작 허용.
- `stop(ws) -> bool` — stop_tx + join.
- `status(ctx, ws)` — `running`/`crashed`/`ready_count`/`running_count`.

thread 본문은 `RunnerLoop::tick` + 500ms `recv_timeout`. tick 안 memory lock 은 *짧은 구간* 만(list → release → dispatch/poll(lock 밖) → re-lock for set_state) — 사용자 CLI 동시 호출과 락 경합 최소화.

## host→plugin 동기 IPC (`HostIpcInjector`)

runner thread 는 off-main 이라 `PluginManager`(App main thread 단독 소유)를 직접 못 부른다. injector 경유: `IpcCommand`+`sync_channel(1)` 을 App IPC 큐에 push → waker 로 App 깨움 → tick 의 routing 이 plugin 에 forward → 응답이 sync_channel 회신 → runner 의 `recv_timeout(5s)`. `Core::set_host_ipc_injector` 가 IPC 시작 직후 1회 등록(boot.rs headless + window_lifecycle.rs gui 양쪽).

## 동기화 primitive 통합

| primitive | 통합 위치 | 결합 |
|-----------|----------|------|
| `Semaphore` | RunnerLoop dispatch | `task.metadata.semaphore = { name, holder? }` |
| `Lease` | RunnerLoop dispatch | `task.metadata.lease = { resource, holder?, ttl_ms?, mode? }` |
| `Barrier` | dispatch/poll | `WaitBarrier { name }` task(DAG 안 명시 gate) |
| `RateLimit` | IPC dispatcher 미들웨어 | `(agent, "ipc_calls")` 호출당 1 차감 |

### dispatch 게이트 (lease → semaphore)

`lease → semaphore` 순서로 점유. 한쪽 점유 후 다음이 Deferred/Err 면 점유 자원 즉시 release(idempotent). dead-lock 회피 — 두 자원 모두 가용일 때만 통과. permit/lease 는 task 가 Succeeded/Failed/Cancelled 로 종결되면 자동 release.

**`holder == task.id` 컨벤션(강제)**: holder 가 task.id 와 다르면 *외부 도구가 직접 acquire 한 것* 으로 간주, 호스트 재시작 정화 대상에서 제외. 외부 점유 회수는 외부 도구 책임.

### 호스트 재시작 정화 + 핸들 영속

`held_permits`/`held_handles` 는 in-memory only이라 재시작 시 비지만, store 의 holders/handle 은 영속이라 leak 가능. runner thread 진입 직전 1회:

- `purge_stale_{semaphore,lease}_holders` — Running task 중 `metadata.*.holder == task.id` 만 release + task=Failed("host restart").
- `reload_persistent_handles`(key `tasty.agent.handle.<task_id>`, workspace scope) — `ShellProcess` 는 `process_alive::is_alive(pid)` 검사(alive 복원 / dead 는 영속 `run_result` 로 정확한 exit_code 마감 또는 Failed). `PolledDispatch`/`BarrierPoll` 은 insert-only 복원(다음 tick poll). PolledDispatch 첫 poll 이 injector 미준비면 `INJECTOR_GRACE_MS=30s` 안에서 Active 유지.

`ReduceImmediate`/`CustomImmediate`/`ImmediateFail` 은 영속 안 함(다음 tick 즉시 흡수 + reload 시 재dispatch side-effect 위험).

### rate_limit 미들웨어

`src/adapters/ipc/handler.rs` 의 미들웨어 체인:

```text
ensure_allowed → check_cap_block → rate_limit_try_consume → record_ipc_call → audit Allow → route
```

차단 시 `-32010 throttled: tokens_left=N` + audit Deny. **면제**(`should_rate_limit`): `Local`(사용자 직접) · host 자기 호출(`_host`) · `telemetry.*`(재귀 폭주 방지) · `agent.rate_limit_*`(자가 회복 경로 — 막히면 영구 차단) · `system.info`. 미등록 `(agent, metric)` 은 면제(opt-in 모델). store 접근 실패는 fail-open(warn 후 통과 — 인프라 고장으로 전 IPC 차단은 과도).

## CLI 예

```sh
tasty agent task-run --workspace-id 1 --action start    # 시작(실행 중이면 no-op)
tasty agent task-run --workspace-id 1                    # 상태 조회
tasty agent task-run --workspace-id 1 --action stop      # 중단(자식 프로세스는 생존)
```

`--action` 은 `clap::ValueEnum { Start, Stop, Status }` — 오타는 CLI 시점 거부.

## 한계

호스트가 ShellProcess spawn 과 watcher 완료 영속 사이에 죽으면 자식이 init(1) reparent 되어 exit_code 손실 → reload 시 `Failed("exit_code unknown")`. (cross-platform 으로 회피 불가.)

## 관련

- [agent-identification](agent-identification.md) — `AgentId` 도출 · [reference/api](../reference/api.md) — agent namespace
