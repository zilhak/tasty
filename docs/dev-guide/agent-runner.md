# Agent task runner

Phase H.F 에서 추가된 *task DAG executor*. workspace 단위로 thread 1개가
`Ready` task 를 자동 dispatch 하고 `Running` task 의 완료를 polling 으로 감지해
state 머신을 진행시킨다.

상태 머신 자체와 영속은 [`tasty-agent`](../../crates/tasty-agent/) 가 담당하고,
실제 *실행* 만 host 측이 위임받는다.

---

## 구성요소

| 파일 | 역할 |
|------|------|
| `crates/tasty-agent/src/runner.rs` | `TaskExecutor` trait + `RunnerLoop::tick` (순수 로직) |
| `crates/tasty-agent/src/platform/process_alive.rs` | cross-platform pid liveness probe |
| `src/core/agent/runner_host.rs` | `HostExecutor` — `TaskExecutor` host 구현. `RunnerContext` (memory + agent_seq + host_ipc injector) |
| `src/core/agent/runner_thread.rs` | `RunnerRegistry` — workspace 별 thread start/stop/status |
| `src/app/ipc/host_call.rs` | `HostIpcInjector` — runner thread 가 plugin IPC 메서드를 동기 호출하는 통로 |
| `src/adapters/ipc/handler/agent/task.rs::handle_task_run` | `agent.task_run` IPC handler |
| `crates/tasty-cli` `agent task-run` | CLI 매핑 |

---

## 모델

### `TaskExecutor` trait

```rust
pub trait TaskExecutor {
    fn dispatch(&mut self, task: &Task) -> Result<DispatchHandle, String>;
    fn poll(&mut self, handle: &DispatchHandle) -> PollOutcome;
}
```

`dispatch` 는 *비차단* 으로 실제 실행을 시작하고 핸들 반환. `poll` 은 *1 tick*
의 현재 상태를 반환 (Active / Done / Failed). 호스트는 polling interval 마다
`RunnerLoop::tick` 한 번씩 호출.

### `DispatchHandle`

`Clone` — RunnerLoop 가 내부 map 에 보관 + executor.poll 호출 시 사본 전달.

- `ClaudeChild { parent_sid, child_index, workspace_id }` — `claude.spawn` 결과.
- `ShellProcess { pid }` — `Run` task 의 자식 프로세스. `Child` 객체는 host
  executor 의 `shell_children: HashMap<u32, Child>` 에 별도 보관 (Child 가
  Clone 안 됨).
- `ReduceImmediate(TaskResult)` / `CustomImmediate(TaskResult)` — dispatch
  시점에 즉시 결과 결정.
- `ImmediateFail(String)` — dispatch 가 실패. transition 표상 *Ready → Failed
  직접 전이 불허* 이므로 *먼저 Running 으로 보낸 후* 다음 tick poll 에서
  `Failed` 로 흡수하기 위한 우회 variant.

### `PollOutcome`

`Active` / `Done(TaskResult)` / `Failed(String)`.

---

## RunnerLoop::tick

```text
1. Running task 순회 → executor.poll(handle)
   - Active: 다음 tick 까지 그대로
   - Done(result): set_result(result) + set_state(Succeeded)
   - Failed(err): set_result({error}) + set_state(Failed{err})
2. Ready task 순회 → executor.dispatch(task)
   - Ok(handle): set_state(Running) + running.insert(id, handle)
   - Err(e): handle = ImmediateFail(e), set_state(Running), 다음 tick 흡수
```

state 전이는 `tasty-agent` 의 `is_valid_transition` 표를 그대로 따름.

---

## HostExecutor 매핑

| TaskCommand | dispatch | poll |
|-------------|----------|------|
| `ClaudeSpawn { prompt, parent_surface, ... }` | `claude.spawn` 동기 IPC → `child_index` 추출 → `ClaudeChild` 핸들. `parent_surface` 가 `None` 이면 Err. | `claude.wait` → `state` 가 `idle` / `needs_input` / `exited` 이면 `Done(final_state)`. `active` 면 `Active`. |
| `Run { command, cwd }` | `std::process::Command::spawn` → pid → `ShellProcess`. 빈 command 면 Err. | `Child::try_wait`: `Some(status)` → exit 0 면 `Done(pid)`, 아니면 `Failed`. `None` → `Active`. |
| `Custom { ipc_method, params }` | host IPC dispatch — 즉시 응답 가정. 응답이 timeout (5s) 안 오면 Err. | dispatch 시점에 `CustomImmediate` 핸들에 결과 박힘 → poll 은 즉시 `Done`. |
| `Reduce { inputs, strategy }` | input task 결과 collect (memory lock) → `reduce_with_custom` 실행 → `ReduceImmediate(result)`. | dispatch 시점 결정 → 즉시 `Done`. |

---

## RunnerRegistry

`Core::agent_runner_registry()` getter 로 접근. workspace 1개당 thread 1개:

- `start(ctx, workspace_id) -> bool` — 이미 실행 중이면 false (idempotent).
  crashed 상태면 정리 후 재시작 허용.
- `stop(workspace_id) -> bool` — stop_tx 보내고 join. 미존재면 false.
- `status(ctx, workspace_id) -> RunnerStatus`
  - `running: bool` — thread 가 살아 있는지
  - `crashed: bool` — panic 후 catch_unwind 가 흡수했는지
  - `ready_count`, `running_count`: 현재 workspace 의 task 카운트

thread 본문은 `RunnerLoop::tick` + 500ms `recv_timeout`. tick 안에서 memory
lock 은 *짧은 구간* 만 — list → release → executor dispatch/poll (lock 바깥) →
re-lock for set_state/set_result. 이는 사용자 CLI 의 `agent.task_create` 같은
동시 호출과 락 경합을 최소화하기 위함.

---

## host→plugin sync IPC dispatch

runner thread 는 *off-main thread* 라 `PluginManager::forward_namespace_call`
(App main thread 단독 소유) 을 직접 호출 못 함. 대신 `HostIpcInjector` 를
거친다:

1. `IpcCommand` 를 `sync_channel(1)` response 채널과 함께 만들어 App 의 IPC
   command 큐에 직접 push.
2. `IpcWaker` 호출 — App main loop 가 깨어남.
3. App tick 에서 `ipc_step_routing` 이 plugin namespace 매칭 → plugin worker
   에 forward.
4. plugin 응답이 sync_channel 으로 회신 → runner thread 의 `recv_timeout(5s)`
   가 깨어나 결과 반환.

`Core::set_host_ipc_injector` 가 Hub::start_ipc 직후 1회 호출. boot.rs (headless)
와 window_lifecycle.rs (gui) 양쪽 모두 등록.

---

## CLI

```sh
# 시작 (이미 실행 중이면 no-op)
tasty agent task-run --workspace-id 1 --action start

# 상태 조회 — 기본
tasty agent task-run --workspace-id 1
# → { "running": true, "crashed": false, "ready_count": 0, "running_count": 2 }

# 중단 (실행 중인 task 의 자식 프로세스는 살아남음 — 사용자가 별 명령으로 정리)
tasty agent task-run --workspace-id 1 --action stop

# 외부 / 수동으로 task 결과 보고
tasty agent task-set-result \
    --workspace-id 1 \
    --id t-... \
    --state succeeded \
    --output '{"x": 1}'
```

`--action` 은 `clap::ValueEnum { Start, Stop, Status }`. 오타는 컴파일/CLI
시점에 거부됨.

---

## 한계 / 향후

- **재시작 후 `ShellProcess` 의 exit_code 미상** — pid liveness 만 검사 가능.
  Child object 가 사라졌으므로 종료 코드/exit status 는 복원 불가. 다음 tick poll
  에서 `process_alive::is_alive` fallback 이 alive/dead 만 판정.
- **`ClaudeChild` reload 시 IPC injector 미준비 가능성** — reload 단계는 *복원만*
  (RunnerLoop.running 에 insert) 하고 즉시 poll 하지 않는다. 다음 정상 tick 에서
  poll. injector 가 그 시점에도 미초기화이면 `PollOutcome::Failed` 로 흡수되어
  task=Failed (R3 정책: 사용자 retry).

barrier / semaphore / lease / rate_limit 와 task 의 자동 통합 (semaphore + lease
gated dispatch) 는 Phase J.A 에 흡수되었다.

### Phase J.A 에서 해결

- **DispatchHandle 영속** — `tasty.agent.handle.<task_id>` 키로 workspace scope
  영속. Immediate*/ImmediateFail 제외. 호스트 재시작 시 `reload_persistent_handles`
  가 `process_alive::is_alive` 로 pid 검사 → alive 복원 / dead Failed 마감.
- **동시성 제어 (lease + semaphore)** — `HostExecutor` 가 `task.metadata.semaphore`
  + `task.metadata.lease` 컨벤션을 dispatch 게이트로. 둘 다 점유한 후 한쪽 실패 시
  즉시 release. 순서 lease → semaphore (R-4 dead-lock 회피).
- **`agent.task_await` blocking** — `TaskWakerHub` (sync_channel + waiters
  HashMap + recv_timeout). 현 state 가 종결이면 즉시 반환. `timeout_ms None|0` =
  무한 대기 (record-level timeout 없음). fire 는 runner_thread tick / Core
  wrapper (set_state, cancel) 모든 경로에서 호출.
- **`OnFailure::Fallback` 동적 task 생성** — `Fallback { task: Option<TaskId>,
  inline: Option<Box<InlineFallbackSpec>> }`. inline 은 main 이 Failed 가 되는
  set_state 분기에서 `TaskStore::create` 가 새 task 발급. metadata.fallback_of
  로 idempotency (반복 Failed → 한 번만 생성).
