# Agent runner primitives — semaphore / barrier / rate_limit 통합

`tasty-agent` 의 동기화·자원 primitive (`Barrier` / `Semaphore` / `RateLimit`) 가
host runner / IPC dispatcher 에 어떻게 결합되는지 정리한다. 본 문서는 *통합
경로* 만 다룬다 — 각 primitive 자체의 IPC / 영속 형식은 `features.md` 참조.

## 1. 사용 표면 요약

| primitive | 통합 위치 | 결합 방식 |
|---|---|---|
| `Semaphore` | RunnerLoop dispatch | `task.metadata.semaphore = { name, holder? }` 컨벤션 |
| `Barrier` | RunnerLoop dispatch/poll | `TaskCommand::WaitBarrier { name }` task |
| `RateLimit` | IPC dispatcher 미들웨어 | `(agent, "ipc_calls")` 1 차감 / 호출 |

## 2. Semaphore-gated dispatch

### 2.1 metadata 컨벤션

Task 생성 시 `metadata.semaphore` 객체를 두면 dispatch 가 자동으로 permit
점유 게이트로 흐른다.

```json
{
  "semaphore": {
    "name": "build_slot",   // SemaphoreStore::create 로 만든 이름
    "holder": "<task.id>"   // optional — 생략 시 task.id 사용
  }
}
```

**holder == task.id 컨벤션 (강제)**

`holder` 가 `task.id` 와 다르면 runner 는 그 항목을 *외부 도구가 직접
acquire 한 semaphore* 로 간주하고 호스트 재시작 시 정화 대상에서 제외한다.
runner 가 점유한 permit 만 회수하기 위함. 외부 도구가 acquire 한 permit 의
회수는 외부 도구의 책임.

### 2.2 RunnerLoop tick 3-way 분기

`TaskExecutor::dispatch` 는 `DispatchOutcome` 3-way 결과를 반환한다:

- `Started(h)` — handle 보관 + Ready → Running 전이.
- `Deferred` — 이번 tick 의 dispatch 불가 (permit 부족 등). state 전이 X.
  task 는 Ready 유지, 다음 tick 재평가.
- `PermanentFail(e)` — 즉시 실패. `ImmediateFail` handle 로 wrapping 되어
  다음 tick poll 에서 Failed 흡수.

### 2.3 permit 회수 시점

- task 가 Succeeded / Failed 로 종결되면 `release_permit` 자동 호출.
- `agent.task_cancel` 로 외부 store 에서 Cancelled 로 직변경된 task 는 매
  tick 의 Cancelled 흡수 arm 에서 `release_permit` + handle drop.

`SemaphoreStore::release` 는 idempotent (holder 부재 시 no-op) — 같은 task 가
여러 경로로 회수되어도 안전.

### 2.4 호스트 재시작 정화

`held_permits` 는 in-memory only — 호스트 재시작 시 비어있다. 한편 store 의
`holders` set 은 영속이라 직전 점유 holder 가 영구 leak 가능. `runner_thread`
의 `run_loop` 진입 직전 `purge_stale_semaphore_holders` 가 1 회 수행:

1. workspace 의 모든 Running task 를 load.
2. `metadata.semaphore.holder == task.id` (위 컨벤션) 인 항목만 정화 대상.
3. 해당 holder 를 `SemaphoreStore::release`.
4. task 자체를 `Failed("host restart")` 로 마감 — DispatchHandle 영속화는
   범위 밖이라 사용자가 `task-retry` 로 정리.

## 3. WaitBarrier task

barrier 통합은 *DAG 안의 명시적 gate* 로 표현한다. metadata 기반 hidden gate
대비 의도가 명확해 추적이 쉬움.

```jsonc
{ "kind": "wait_barrier", "name": "phase_1_done" }
```

- `dispatch` → `DispatchHandle::BarrierPoll { workspace_id, name }`.
- `poll` 매 tick:
  - `BarrierState::Open` → `PollOutcome::Active`
  - `BarrierState::Closed` → `Done({ barrier, count_signaled, count_required })`
  - `BarrierState::TimedOut` → `Failed("barrier '...' timed out")`

timeout 단일 출처 = `BarrierStore::create` 시점의 `timeout_ms`. `WaitBarrier`
자체에는 timeout 필드를 두지 않는다 (YAGNI).

## 4. IPC dispatcher rate_limit 미들웨어

`src/adapters/ipc/handler.rs::handle_with_caller` 의 미들웨어 체인 중 cap_block
검사 직후에 1단계 추가됨:

```text
ensure_allowed → check_cap_block → rate_limit_try_consume → record_ipc_call → audit Allow → route
```

차단 결과: `-32010 throttled: tokens_left=N.NN` + audit Deny.

### 4.1 면제 규칙 (`should_rate_limit`)

다음 경우 throttle 검사 자체를 건너뛴다:

- `CallerContext::Local` — 사용자가 직접 CLI/network 호출. 무제한.
- `Agent` 중 `agent_id().is_host()` (= `_host`) — 호스트 자기 호출.
  `telemetry::record_ipc_call` 의 `_host` 제외 정책과 일관.
- method 가 `telemetry.*` 로 시작 — 재귀 폭주 방지 (record_ipc_call 자체가
  telemetry 호출을 만듦).
- method 가 `agent.rate_limit_*` 로 시작 — **자가 회복 경로**. 이게 막히면
  한 번 throttle 된 agent 가 자기 한도를 풀 수 없어 영구 차단된다.
- method == `system.info` — 단순 상태 조회.

### 4.2 throttled 호출의 telemetry 정책

throttled 분기는 `record_ipc_call` 을 호출하지 *않는다*. 즉 throttle 로 차단된
호출은 `ipc_calls` telemetry 이벤트로 카운트되지 않는다. 운영자가 throttle
폭주를 추적해야 할 때는 `RateLimitStore` 의 `throttled_count` 필드를 쓴다
(`rate_limit.rs` 의 `RateLimit::throttled_count`). 이는 의도된 정책 — throttle
자체로 차단됐다는 사실은 audit Deny 로 기록되므로 중복 카운팅을 피한다.

### 4.3 미등록 (agent, metric) = throttle 면제

`RateLimitStore::try_consume` 의 unregistered 분기는 항상 `allowed=true,
tokens_left=infinity` 를 반환한다. 의도된 동작 (opt-in 모델) — agent 가 자기
rate_limit 을 등록하지 않으면 throttle 자체가 적용되지 않는다. 운영자가 *전체
agent throttle* 을 강제하려면 모든 agent 에 대해 명시적으로 `rate_limit_set`
을 호출해야 한다.

### 4.4 fail-open 정책

`rate_limit_try_consume` 의 store 접근이 실패하면 미들웨어는 `tracing::warn!`
로 로그만 남기고 통과시킨다. rate_limit 인프라 자체가 망가졌다고 모든 IPC 를
차단하는 것은 과도하다는 판단 — 운영자가 로그로 인지 후 복구.

## 5. Lease-gated dispatch (Phase J.A)

### 5.1 metadata 컨벤션

```jsonc
{
  "lease": {
    "resource": "file:/etc/example",   // 필수 — 협조적 점유 키
    "holder": "<task_id>",              // 옵션 — 미명시 시 task.id 자동
    "ttl_ms": 60000,                    // 옵션 — 만료 후 lazy evict
    "mode": "block"                     // 옵션 — "block" (default, Deferred 매핑)
                                        //        "fail" (PermanentFail 매핑)
  }
}
```

semaphore 와 같은 정책: `holder == task.id` 컨벤션을 강제 (호스트 재시작
정화의 대상 식별용). 외부 도구가 임의 holder 로 acquire 한 lease 는
`purge_stale_lease_holders` 의 회수 대상이 아니다.

### 5.2 dispatch 게이트 순서

`lease → semaphore` 순서로 시도. 한쪽 점유 후 다음 게이트가 Deferred / Err 면
점유한 자원 즉시 release (idempotent). 통합 dead-lock 회피 (R-4): 두 자원
모두 가용한 시점에서만 dispatch 통과.

### 5.3 호스트 재시작 정화

`runner_thread::purge_stale_lease_holders` 가 workspace runner 시작 직전
1회 수행. Running task 중 `metadata.lease.holder == task.id` 만 정화 대상 —
store 에서 release + task=Failed("host restart") 마감.

## 6. DispatchHandle 영속 (Phase J.A)

### 6.1 영속 key + 형식

key: `tasty.agent.handle.<task_id>` (workspace scope). 값은
`DispatchHandle` 의 serde JSON (`tag="kind", content="data"` adjacently-tagged).

### 6.2 영속 대상

- `ClaudeChild`, `ShellProcess`, `BarrierPoll`: 영속.
- `ReduceImmediate`, `CustomImmediate`, `ImmediateFail`: 영속 *안 함* —
  다음 tick poll 에서 즉시 흡수되므로 영속해 둘 의미가 없고, reload 시
  재dispatch 되어 side-effect 위험.

### 6.3 reload 흐름

`reload_persistent_handles` 가 workspace runner 시작 직전 호출:

1. task state 가 Running 이 아니면 stale (영속만 제거, state 보존). R-1 회피.
2. `ShellProcess { pid }`: `process_alive::is_alive(pid)` 검사.
   - alive → 복원 (다음 tick poll 의 cell miss → `process_alive` fallback).
   - dead + `run_result.<task_id>` 영속 존재 → 정확한 exit_code 로 Succeeded /
     Failed 마감 (K.A-1 precise).
   - dead + run_result 없음 → task=Failed("host restart: pid X died (exit_code
     unknown)") + evict.
3. `ClaudeChild` / `BarrierPoll`: 복원 (insert only — 즉시 poll 안 함).
   다음 정상 tick 에서 poll. ClaudeChild 의 첫 poll 이 injector 미준비여도 K.A-2
   grace (`INJECTOR_GRACE_MS = 30s`) 안에서는 Active 유지 — deadline 도래 후이면
   `Failed("injector grace expired")`.

### 6.4 evict 시점

`HostExecutor::release_permit` (task 종결) 안에서 자동. ws 는 별도
`held_handles: HashMap<TaskId, u32>` map 으로 추적 (handle 자체에서 ws 식별
불가 — ShellProcess { pid } variant 에 workspace_id 없음).
