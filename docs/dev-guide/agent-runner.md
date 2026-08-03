# Agent task runner

workspace 단위 thread 1개가 `Ready` task 를 자동 dispatch 하고 `Running` task 완료를 polling 으로 감지해 state 머신을 진행시키는 *task DAG executor*. 상태 머신·영속은 [`tasty-agent`](../../crates/tasty-agent/) 가, 실제 *실행* 만 host 가 위임받는다. IPC/CLI 표면 명세는 [reference/api](../reference/api.md)("agent" namespace).

## 구성

| 파일 | 역할 |
|------|------|
| `crates/tasty-agent/src/runner.rs` | `TaskExecutor` trait + `RunnerLoop::tick`(순수 로직) |
| `crates/tasty-agent/src/platform/` | cross-platform pid liveness probe(`process_alive`) |
| `src/core/agent/runner_host.rs` | `HostExecutor` — `TaskExecutor` host 구현 + `RunnerContext`(memory + agent_seq + host_ipc injector) |
| `src/core/agent/runner_thread.rs` | `RunnerRegistry` — workspace 별 thread start/stop/status + 재시작 정화 |
| `src/adapters/ipc/host_call.rs` | `HostIpcInjector` — runner thread 가 plugin IPC 를 동기 호출하는 통로 |
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

`PolledDispatch { workspace_id, poll_method, poll_params, state_field, terminal_states, interval_ms, deadline_ms }`(범용 폴링 — dispatch 시점에 완성된 `poll_params` 로 terminal 상태 도달까지 `poll_method` 반복 호출) · `ShellProcess { pid }`(`Run` task 자식; `Child` 객체는 Clone 불가라 executor 의 `shell_children` map 에 별도 보관) · `BarrierPoll { workspace_id, name }` · `ReduceImmediate`/`CustomImmediate`/`ImmediateFail`(dispatch 시점 즉시 결정) · `AwaitExternal { wait_key, deadline_ms }`(push-kind 완료 전략, 아래 참조 — `poll` 은 계약대로 **항상 Active**, 종결은 외부에서 store 를 직접 전이시킨다). `deadline_ms` 는 dispatch 시점 `now + timeout_ms` — handle 자체에 실려 영속되므로, 이 handle 을 만든 `hook_task_waits` 매핑(비영속)이 재시작으로 사라져도 재시작 후 reload 가 독자적으로 만료 판정을 할 수 있다(아래 "호스트 재시작 정화 + 핸들 영속" 참조). 이 필드 도입 이전에 영속된 구 포맷은 `#[serde(default)]` 로 `0`(=즉시 만료)이 된다.

### HostExecutor 매핑

| TaskCommand | dispatch | poll |
|-------------|----------|------|
| `Run { command, cwd }` | `Command::spawn`(stdout/stderr `Stdio::piped()`) → pid → `ShellProcess`. 빈 command Err. stdout/stderr 를 각각 별도 드레인 스레드로 즉시 읽기 시작(파이프 교착 방지 — 아래 "출력 캡처" 참조) | watcher thread 의 `child.wait()` 결과 cell 조회 → exit 0 이면 두 드레인 스레드를 join 해 캡처 결과를 실은 Done / 아니면 Failed(캡처 결과를 에러 메시지에 포함) |
| `Custom { ipc_method, params, poll: None }` | host IPC dispatch(timeout 5s) → 등록된 완료 판정 전략 중 `ipc_method` 를 `default_for_methods` 로 지목한 전략이 있으면 그 kind 를 채택(아래 "완료 판정 전략 레지스트리" 결정 6 — poll 이면 `PolledDispatch`, push 면 아래 push 행과 동일), 없으면 `CustomImmediate` | 매칭된 kind 에 따라 poll/push 행과 동일 / 아니면 즉시 Done |
| `Custom { ipc_method, params, poll: Some(PollSpecRef::Inline(spec)) }` | host IPC dispatch → `map_from_request`/`map_from_response` 로 `poll_params` 완성 → `PolledDispatch` | `poll_method` 호출 → `state_field` 가 `terminal_states` 중 하나면 Done(응답 전체가 산출물) / 아니면 Active(`deadline_ms` 초과 시 Failed) |
| `Custom { ipc_method, params, poll: Some(PollSpecRef::Named{strategy}) }`, poll-kind | 완료 판정 전략 레지스트리에서 `strategy` 를 이름 해석(`resolve_strategy`) → 얻은 `PollSpec` 으로 위 Inline 행과 동일 처리. 미등록/비활성이면 해석 실패 → dispatch 자체가 `PermanentFail`(Running 진입 전에 드러남) | 위와 동일 |
| `Custom { ipc_method, params, poll: Some(PollSpecRef::Named{strategy}) }`, push-kind | `dispatch_push_strategy` — 원 dispatch `params.surface_id` 대상 surface 에 `notify_via` 훅 핸들러를 `hook.set(..., once: true)` 로 1 회성 등록해 `hook_id` 획득 → `RunnerContext.hook_task_waits` 에 `(workspace_id, task_id, deadline)` 등록 → `AwaitExternal`. `surface_id` param 이 없으면 `PermanentFail` | 항상 Active(계약) — 종결은 `PendingHostEvent::HookFired` 소비부(`Core::resolve_hook_task_wait`, exit code 로 성공/실패 분기)와 timeout 안전망(`runner_thread::expire_overdue_hook_waits`)이 담당 |
| `Reduce { inputs, strategy }` | input 결과 collect → `reduce_with_custom` → `ReduceImmediate`. `inputs` 는 Task DAG 의 암묵적 의존성(`TaskGraph`, `crates/tasty-agent/src/task/graph.rs`)이라 dispatch 시점엔 이미 전부 종결(terminal) 상태다 — `Ready` 로 올라오기 전에 readiness 평가가 그 종결을 강제한다 | 즉시 Done |
| `WaitBarrier { name }` | `BarrierPoll` | `Open`→Active / `Closed`→Done / `TimedOut`→Failed |

> 자식 에이전트(예: 미래의 `claude.spawn` 완료 판정)나 셸 명령 완료(현재: `host/command-completed`)는 이 범용 `Custom { poll }` 메커니즘의 사용자다 — 코어는 특정 에이전트를 모른 채 임의 IPC dispatch→폴링/훅-보고를 표현한다. CLI auto_wait(`AutoWaitDecl`/`PollingDecl`)와 동형 스펙으로 폴링 semantics 를 통일한다. 예:
> ```json
> {"kind":"custom","ipc_method":"surface.send","params":{"surface_id":7,"text":"npm test"},
>  "poll":{"strategy":"host/command-completed"}}
> ```

### `Run` 출력 캡처

`Run` 은 Surface(Tab)를 만들지 않는 bare subprocess다 — argv 를 그대로 `Command::spawn` 에 넘기고(셸 word-splitting 없음), exit code 도 명령 자신의 것이다. tty 가 필요한 명령은 지원 대상이 아니다(그건 `pty.*` primitive 의 몫).

- **드레인 스레드 필수**: `Stdio::piped()` 만 붙이고 파이프를 읽지 않은 채 `child.wait()` 하면 자식이 OS 파이프 버퍼(플랫폼별 16~64KB)를 채우고 block, 부모는 그 자식의 종료를 기다리므로 교착한다. dispatch 가 stdout/stderr 를 각각 별도 스레드(`agent-shell-{stdout,stderr}-pid<N>`)로 즉시 드레인 시작하고, watcher(`agent-shell-watcher-pid<N>`)는 `child.wait()` 후 두 드레인 스레드를 join 한다 — task 당 스레드 3개(watcher + drain 2개).
- **스트림당 64KiB tail**: 각 드레인 스레드는 EOF 까지 계속 읽되 마지막 64KiB 만 보관한다(head 는 버림). 상한을 넘기면 `truncated: true` + `dropped_bytes` 를 함께 기록. 64KiB × 2스트림인 이유는 캡처 결과가 `run_result` 로 memory store 값 하나(1MiB 상한)에 JSON 직렬화되기 때문 — ANSI escape 팽창까지 고려한 최악의 경우도 1MiB 아래.
- **성공/실패 모두에 담김**: 성공(exit 0)은 `TaskResult.output = {"pid", "stdout": {"text","truncated","dropped_bytes"}, "stderr": {...}}`. 실패(비0 exit)는 `PollOutcome::Failed` 가 문자열 하나만 나르는 계약이라, 캡처한 stdout/stderr tail 을 에러 메시지 본문에 그대로 이어붙인다 — `cargo build` 같은 명령의 컴파일 에러 본문도 이 경로로 드러난다.
- **알려진 한계**: tail 전용이라 긴 빌드의 *첫* 에러는 놓칠 수 있다(요약/실패 지점은 보통 출력 뒤쪽). ANSI 는 벗기지 않고 보존한다.

## 완료 판정 전략 레지스트리 (`src/completion_strategy/`)

`Custom.poll` 이름 참조(`PollSpecRef::Named`)와 결정 6(`default_for_methods`) 이 가리키는 대상 — "임의 IPC dispatch 가 끝났는지"를 이름으로 등록해두는 독립 레지스트리다. `src/hook_handler/`(공유 훅 핸들러 레지스트리) 를 정본 템플릿으로 **형태만** 미러링한다 — 3출처 병합(host 내장 TOML + plugin manifest + user config `~/.tasty/completion-strategies.toml`), patch semantics(Host→Plugin→User, `Some` 필드만 override), id 규약(`<owner>/<short>`, `host`/`<plugin_id>`/`user`), 전역 싱글턴 `global()`. `HookHandlerId`(push 형이 참조) 외에는 훅 핸들러의 타입을 import 하지 않는다.

전략 종류(`CompletionStrategyKind`):
- **poll**: `tasty-agent::PollSpec` 그대로 재사용.
- **push**: `notify_via: HookHandlerId` + 필수 `timeout_ms`(보고 유실 시 task 가 영구 Running 에 남지 않도록 하는 안전망). `notify_via` 는 등록 시점에 존재·owner(자기 자신 또는 `host`) 를 검증한다.

`resolve_poll_spec(id)` 은 poll-kind 만 반환(push 는 `NotPollKind` 에러) — poll spec 만 필요한 소비자용. `resolve_strategy(id)` 는 kind-agnostic — `Custom` dispatch(runner_host.rs)처럼 poll/push 를 모두 다뤄야 하는 호출부가 쓴다.

**push 완료 신호 소비 배선**: push-kind dispatch 는 `notify_via` 가 가리키는 훅 핸들러를 원 dispatch `params.surface_id` 대상 surface 에 `hook.set(event: "command-completed", once: true)` 로 1 회성 등록해 `hook_id` 를 얻고, `RunnerContext.hook_task_waits`(`Arc<HookTaskWaits>` — runner thread 가 `Core` 를 거치지 않고 `task_waker_hub` 와 동형으로 직접 공유)에 `(workspace_id, task_id, deadline)` 로 등록한 뒤 `AwaitExternal` 로 전이한다. 실제 종결은 두 경로:
- **정상 보고**: 그 훅이 실제로 발화하면 `PendingHostEvent::HookFired` 소비부(`app/dispatch/host_events.rs::resolve_hook_fired_task_waits` → `Core::resolve_hook_task_wait`)가 hook_id 로 대기 task 를 찾아 마감한다 — 실제 관측된 exit code(`CommandCompleted` 만 보유)가 있으면 `0`/없음 → Succeeded, 비-0 → Failed(그 코드를 실은 error 메시지). exit code 개념이 없는 push 신호는 Succeeded.
- **timeout 안전망**: `runner_thread::expire_overdue_hook_waits` 가 매 tick `HookTaskWaits::sweep_expired` 로 deadline 지난 항목을 강제 Failed 마감한다(워크스페이스 무관 전역 sweep — 어느 runner thread 든 편승 가능).

이 기전으로 등록되는 오늘 유일한 push 전략이 `host/command-completed`(아래 참조)다. `notify_via` 훅 핸들러의 실제 action(현재: `notification.create` 알림)은 task 완료 판정과 무관한 부가 효과일 뿐 — 판정은 hook_id 매칭만으로 이뤄진다.

네임스페이스 제한(결정 2): plugin 소유 전략의 `poll_method`/`default_for_methods` 는 자기 IPC namespace(`<plugin_id>.*`) 만 가리킬 수 있고, host/user 소유는 어떤 plugin namespace 도 가리킬 수 없다(`_host` 권한 우회 방지) — `tasty_ipc::method_meta::is_registered_plugin_prefix` 로 검증.

결정 6(`default_for_methods`, 역방향 소유): 매니페스트에 메서드 단위 선언 축이 없어 전략이 자기가 기본이 될 IPC 메서드 목록을 든다. 여러 활성 전략이 같은 메서드를 지목하면 정렬 승자(priority↑ → owner tie-break user>plugin>host → id)가 채택되고 패자는 warn.

IPC/CLI: `completion_strategy.list`(전 범위 조회, 비활성 포함) / `tasty completion-strategy list`. reload/dispatch 대응물은 없다(user config 재로드 미노출, "발화" 개념 없음). 내장 host 기본값은 `src/completion_strategy/defaults/default-completion-strategies.toml`:

- `host/command-completed`(결정 7) — OSC 133 셸 통합 기반 push 전략. `notify_via = "host/command-completed"` 는 `src/hook_handler/defaults/default-hook-handlers.toml` 에 등록된 훅 핸들러를 가리킨다(둘 다 host defaults 로 함께 설치되므로 항상 존재). `timeout_ms = 300000`(5분). 전제: 대상 surface 가 OSC 133(셸 통합 스크립트)을 로드하고 있어야 발화한다 — 미로드 surface 를 대상으로 `hook.set --event command-completed`(이 전략의 내부 dispatch 경로 포함)를 걸면 거부는 아니고 warn 로그만 남는다(`shell_integration_boundary_seen`, 시간 기반 추정이라 오탐 가능). `Custom` task 의 dispatch `params` 는 `surface_id` 를 포함해야 한다(예: `surface.send`).

## RunnerRegistry

`Core::agent_runner_registry()` 로 접근. workspace 1개당 thread 1개:

- `start(ctx, ws) -> bool` — 이미 실행 중이면 false(idempotent). crashed 면 정리 후 재시작 허용.
- `stop(ws) -> bool` — stop_tx + join.
- `status(ctx, ws)` — `running`/`crashed`/`ready_count`/`running_count`.

thread 본문은 `RunnerLoop::tick` + 500ms `recv_timeout`. tick 안 memory lock 은 *짧은 구간* 만(list → release → dispatch/poll(lock 밖) → re-lock for set_state) — 사용자 CLI 동시 호출과 락 경합 최소화.

`agent.task_run`(start/stop/status)은 `METHOD_TABLE` 에 `plugin(&[AgentManage])` 로 등록돼 있다 — 호스트가 재시작 시 runner 를 자동으로 켜지 않으므로(아래 "재시작 계약"), plugin 이 자기 workspace 의 runner 를 스스로 되살릴 수단이 필요하기 때문이다. `task_set_result` 만 여전히 local-only.

## 재시작 계약

**자동 시작은 하지 않는다.** 호스트 재시작 후 어떤 workspace 의 runner thread 도 자동으로 켜지지 않는다 — `agent.task_run --action start` 로 수동(또는 plugin) 재개해야 한다. 대신 다음 두 가지를 보장한다:

1. **재시작 정화는 부팅 시 1회, runner 없이도 수행한다.** `purge_stale_agent_state_on_boot`(`Core`, `src/core/mod.rs`)가 headless(`src/boot.rs`, host IPC injector 등록 + `CoreState` 확보 직후)와 GUI(`src/app/boot_machine.rs::finish_boot`, 첫 윈도우 등록 직전) 양쪽 부팅 경로에서 호출된다. 라이브 `CoreState.workspaces` 전부에 대해 아래 "호스트 재시작 정화 + 핸들 영속" 절의 3종 세트(`purge_stale_semaphore_holders`/`purge_stale_lease_holders`/`reload_persistent_handles`)를 수행하고, `reload_persistent_handles` 가 되살린 handle 목록은 버린다(이 시점엔 그걸 넘겨받아 poll 할 runner 가 없다 — 다음 수동 start 가 다시 reload 한다). task 가 없는 workspace 는 각 정화 함수가 candidates 없음으로 조기 반환하므로 실질적으로 no-op — "라이브 workspace ∩ task 보유 workspace" 교집합과 동치. 여러 번 호출해도 안전(idempotent): `alive` 분류는 부수효과가 없고, `dead`/`stale`/`precise` 분류는 이미 정리된 뒤엔 대상이 남지 않는다.
2. **정지 상태는 조회로 드러난다.** `task_run --action status` 뿐 아니라 `task_list`/`task_graph` 응답에도 `runner: { running, crashed, ready_count, running_count }` 를 동반한다 — runner 가 꺼져 있어도(`running: false`) `ready_count`/`running_count` 는 store 를 직접 조회한 실제 값이라, "비-terminal task 는 있는데 아무도 안 돌리고 있다"가 이 응답만으로 드러난다. `task_get` 응답은 task 가 `AwaitExternal` handle 로 외부 신호를 기다리는 중이면 `awaiting_external: { wait_key, deadline_ms }` 를 함께 실어 "그냥 running" 과 구분한다(`AwaitExternal` 의 poll 은 계약상 항상 Active 라 state 만으로는 대기 이유를 알 수 없다). CLI(`tasty agent task-{list,get,run}`)는 이 값들을 사람이 바로 읽는 텍스트로 렌더한다(`crates/tasty-cli/src/format.rs`) — runner 가 멈춰 있고 대기 중인 task 가 있으면 재개 커맨드까지 안내 문구로 보여준다.

`hook_task_waits`(hook_id → task_id 매핑)는 여전히 **비영속**(프로세스 메모리 전용)이다 — 재시작하면 사라진다. 그래서 재시작 후 `AwaitExternal` task 는 **훅으로는 깨어날 수 없고**, 그 handle 에 실린 `deadline_ms`(위 참조)로만 마감된다: reload 시점에 이미 만료된 handle 은 즉시 `Failed`, 아직이면 그대로 복원되지만 이후 그 프로세스가 계속 살아있는 동안은(`AwaitExternal` poll 이 항상 Active 라 tick 이 deadline 을 검사하지 않음) 다음 재시작의 reload 가 다시 판정할 때까지 마감되지 않는다 — "재시작을 한 번 더 거쳐야 완전히 청소된다"는 절충이다.

### 자동 GC

`purge_stale_agent_state_on_boot`(`src/core/agent/runner_thread.rs`)의 같은 루프 안에서, 위 3종 세트 정화 직후 `gc_stale_tasks(ctx, workspace_id)` 가 한 번 더 돈다 — task 삭제 경로(`agent.task_delete`/`agent.task_purge`, `crates/tasty-agent/src/task/store.rs::{delete_checked,plan_sweep,apply_sweep_plan}`)와 정확히 같은 참조 안전 로직을 태우는 자동 스윕이다.

- **임계값**: `AGENT_TASK_GC_MIN_AGE_MS`(`runner_thread.rs`, 잠정 7일) — 상태와 무관하게 `now - 기준시각 >= 임계값` 인 task 가 후보. 기준시각은 terminal task 는 `finished_at`, 그 외(`waiting`/`ready`)는 `created_at`. 값 자체는 provisional — 실사용 데이터가 쌓이면 재검토 대상.
- **상태를 terminal 로 제한하지 않는 이유**: 방치된 `waiting` task(예: 입력이 끝나지 않는 `Reduce`)를 terminal-only 로 제약하면 영원히 못 지우고, 그게 참조로 자기 입력들을 붙잡아 그 입력들도 영영 GC 대상에서 빠진다. `running` 은 `plan_sweep` 이 항상 후보에서 제외하므로 별도 처리가 필요 없다.
- **`PutOpts.expires_at` 류의 memory 자체 TTL 은 쓰지 않는다** — TTL 만료는 참조 무결성·상태 검사를 완전히 우회한 채 그냥 지워버려, dangling 참조·자원 누수를 다시 끌어들이기 때문이다. 항상 `plan_sweep`(순수 함수, 후보 선정) → `apply_sweep_plan`(실제 삭제) 을 거치고, 실제로 지워진 task 마다 `tasty.agent.handle.<id>`/`tasty.agent.run_result.<id>` side-key 도 `evict_task_side_keys` 로 정리한다.
- `plan_sweep` 은 fixed-point 로 "후보 집합 밖에서 참조되는 task"만 반복 제외한다 — 즉 서로를 참조하는 방치 task 끼리는(예: `waiting` `Reduce`(X)가 그 input(Y)을 참조) 후보 집합 안에서 함께 드레인되고, 후보 밖의 살아있는 task 가 참조하는 대상만 보존(`retained`)된다.

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

`held_permits`/`held_handles` 는 in-memory only이라 재시작 시 비지만, store 의 holders/handle 은 영속이라 leak 가능. `purge_and_reload_on_restart`(`src/core/agent/runner_thread.rs`)로 묶여 있고, **runner thread 없이도** 호출 가능하다 — 부팅 경로(위 "재시작 계약")와 `run_loop` 진입부(수동/plugin start) 양쪽이 이 함수 하나를 공유한다:

- `purge_stale_{semaphore,lease}_holders` — Running task 중 `metadata.*.holder == task.id` 만 release + task=Failed("host restart").
- `reload_persistent_handles`(key `tasty.agent.handle.<task_id>`, workspace scope) — `ShellProcess` 는 `process_alive::is_alive(pid)` 검사(alive 복원 / dead 는 영속 `run_result` 로 정확한 exit_code 마감 또는 Failed). `PolledDispatch`/`BarrierPoll` 은 insert-only 복원(다음 tick poll). PolledDispatch 첫 poll 이 injector 미준비면 `INJECTOR_GRACE_MS=30s` 안에서 Active 유지. `AwaitExternal { deadline_ms, .. }` 은 `deadline_ms` 가 이미 지났으면 즉시 `Failed`(구 포맷도 `deadline_ms` 기본값 0 이라 이 분기), 아직이면 insert-only 복원 — 단 poll 이 절대 관여하지 않는 계약이라 다음 재시작 전까지는 deadline 이 재판정되지 않는다(위 "재시작 계약" 참조).

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

```sh
tasty agent task-delete --workspace-id 1 --id t-...              # 참조 있으면 거부 + 참조자 목록
tasty agent task-delete --workspace-id 1 --id t-... --cascade    # 참조자까지 함께 삭제
tasty agent task-purge --workspace-id 1 --states succeeded,failed --older-than-ms 604800000 --dry-run
```

## 한계

호스트가 ShellProcess spawn 과 watcher 완료 영속 사이에 죽으면 자식이 init(1) reparent 되어 exit_code 손실 → reload 시 `Failed("exit_code unknown")`. (cross-platform 으로 회피 불가.)

같은 이유로 **캡처한 stdout/stderr 도 유실된다** — 자식은 호스트 재시작 후에도 살아남지만(수명 계약은 그대로 유지), 파이프를 들고 있던 드레인 스레드는 호스트와 함께 사라지므로 그 사이의 출력은 다시 읽을 방법이 없다. 장시간 작업(빌드/배포)의 결과 보존이 중요해지면 `pty.*` 기반 별도 경로가 더 맞다.

## 관련

- [agent-identification](agent-identification.md) — `AgentId` 도출 · [reference/api](../reference/api.md) — agent namespace
