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

- **handle 영속 없음** — runner thread 메모리에만. 호스트 재시작 시 Running
  task 는 `Unknown` 으로 fallback (현재는 자동 표시 X — 사용자가 `task-retry`
  로 정리). pid liveness 복원은 `process_alive::is_alive` helper 준비됨, 본
  phase 에서는 호출 X.
- **동시성 제어 없음** — 한 tick 의 모든 Ready 를 한꺼번에 dispatch. 100 개
  병렬 claude.spawn 은 별 phase 의 semaphore 통합으로 조절 예정.
- **agent.task_await blocking 미지원** — 현재는 단순 `task_get` alias.
- **`OnFailure::Fallback` 동적 task 생성 X** — fallback task 는 사전에 존재해야
  동작.

barrier / semaphore / lease / rate_limit 와 task 의 자동 통합은 *별 phase* 의
주제.
