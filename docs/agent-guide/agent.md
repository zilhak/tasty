# 다중 에이전트 협업 (`agent.*`)

여러 AI 에이전트가 같은 Tasty 인스턴스를 공유할 때 쓰는 협업 primitive 6 종.

| primitive | 역할 |
|---|---|
| **Task DAG** | 의존성을 가진 작업 그래프 + state 머신 |
| **Barrier** | N 회 signal 모이면 닫히는 게이트 (timeout 가능) |
| **Semaphore** | N permit 동시 점유 — 같은 holder 재acquire는 idempotent |
| **Lease** | 협조적(advisory) 자원 점유 마커 + TTL |
| **Reducer** | N 개 task 결과를 한 값으로 합성 (4 in-process + 1 custom shell) |
| **Rate-limit** | (agent, metric) 쌍에 대한 token bucket 시간당 비율 제한 |

모든 메서드는 `AgentManage` 권한 필요. 영속은 `tasty.agent.*` prefix 의 `tasty-memory` 키 — task/barrier/semaphore/lease 는 `workspace:<id>` scope, rate-limit 은 `Global` scope.

## 단일 namespace

`task` / `barrier` / `semaphore` / `lease` / `reducer` / `rate_limit` 6 개 namespace 로 쪼개는 대신 **`agent.*` 단일 namespace + `<verb>_<modifier>` 패턴** 으로 통일.

```
agent.task_{create,list,get,await,cancel,retry,graph}
agent.barrier_{create,signal,await,state}
agent.semaphore_{create,acquire,release}
agent.lease_{acquire,release,list}
agent.task_reduce
agent.rate_limit_{set,list,remove,status}
```

권한 한 묶음(`AgentManage`)으로 협업 primitive 전부를 grant/revoke 할 수 있게 하는 게 목적. plugin 매니페스트에서 `AgentManage` 하나만 선언하면 모든 primitive 사용 가능.

## Poll-based 모델

본 phase 5 까지는 **blocking await 가 아니라 polling 모델**:

- `agent.task_await`, `agent.barrier_await` 는 "현재 상태 즉시 응답" 으로 동작 (poll-based).
- 호출자가 terminal/closed 상태가 아니면 반복 호출한다.
- 진정한 blocking + queue/wakeup 은 scheduler 도입 시 long-poll 로 분기.

이렇게 한 이유: blocking await 는 lock-up/dead-lock 위험을 안고 가는데, 본 단계의 primitive 검증에는 polling 만으로 충분히 의미가 있다.

---

## 1. Task DAG (Phase 5.1)

의존성을 가진 작업 그래프. 사이클은 `create()` 시점에 거부, 임의 task 상태 변화 시 transitive downstream 을 자동 재평가.

### TaskState (8 종)

| 상태 | 의미 |
|---|---|
| `waiting` | 의존성 미충족 (dep 중 미완료 존재) |
| `ready` | 모든 dep 가 succeed (또는 fallback 으로 effective-succeed) — 호스트가 실제 실행 트리거 |
| `running` | 진행 중 |
| `succeeded` | 정상 완료 |
| `failed { error }` | 실패 |
| `cancelled` | 명시적 cancel |
| `skipped` | 의존성 실패로 자동 cascade |
| `unknown` | 재시작 후 running 이던 task — 사용자가 retry/cancel 결정 필요 |

### TaskCommand (4 종, `kind` discriminator)

- `claude_spawn` — `claude.spawn` 위임 (직접 실행이 아닌 host 측이 wire 함)
- `run` — 터미널에서 명령 실행
- `custom` — 임의 IPC 위임. caller 권한 검사 별도
- `reduce` — `agent.task_reduce` 와 같은 합성 동작을 task 화

### OnFailure (3 종)

- `abort` (기본) — main 실패 시 downstream 전부 `skipped` cascade
- `continue_downstream` — 실패를 성공처럼 취급 (downstream `ready`)
- `fallback { task: <id> }` — main 실패 시 명시한 fallback task 가 자동 `ready`. fallback 의 succeed/fail 도 main downstream 평가에 반영. fallback task 는 사전에 `task_create` 로 등록되어 있어야 한다

### IPC

| method | 요약 |
|---|---|
| `task_create` | `{ workspace_id, name, command, depends_on?, on_failure?, metadata? }` |
| `task_list` | `{ workspace_id, state? }` — `state` 로 필터 |
| `task_get` | `{ workspace_id, id }` |
| `task_await` | poll-based (현 단계 = `task_get`) |
| `task_cancel` | terminal 이면 `-32008`. downstream cascade 함께 반환 |
| `task_retry` | `Failed/Cancelled/Skipped/Unknown` 만 허용. `reset_downstream?` |
| `task_graph` | `format ∈ {json, dot}` — DAG 시각화 |

### CLI

```
tasty agent task-create --workspace-id <id> --name <n> --command @spec.json \
                       [--depends-on T1,T2] [--on-failure abort|continue_downstream|fallback:T3] \
                       [--metadata @meta.json]
tasty agent task-list   --workspace-id <id> [--state <s>]
tasty agent task-get    --workspace-id <id> --id <T>
tasty agent task-await  --workspace-id <id> --id <T>
tasty agent task-cancel --workspace-id <id> --id <T>
tasty agent task-retry  --workspace-id <id> --id <T> [--reset-downstream]
tasty agent task-graph  --workspace-id <id> [--format json|dot]
```

`--command` / `--metadata` 는 인라인 JSON 또는 `@path` (파일 로드).

---

## 2. Barrier (Phase 5.2)

N 회 `signal` 이 모이면 닫히는 게이트. `timeout_ms` 가 지나면 `timed_out` 으로 lazy 전이 (별도 스레드 없음 — signal / state / list 호출 시점에 도장 찍기).

| 상태 | 의미 |
|---|---|
| `open` | signal 미달 |
| `closed` | `count_required` 도달 |
| `timed_out` | timeout 경과 |

### IPC

| method | 요약 |
|---|---|
| `barrier_create` | `{ workspace_id, name, count_required≥1, timeout_ms? }` |
| `barrier_signal` | `count_signaled++`. 도달 시 `closed`. timeout 지났으면 거부 |
| `barrier_await` | poll-based 상태 조회 (현 단계 = `barrier_state`) |
| `barrier_state` | 조회 시 timeout 도장 자동 적용 |

### CLI

```
tasty agent barrier-create --workspace-id <id> --name <n> --count-required <N> [--timeout-ms <ms>]
tasty agent barrier-signal --workspace-id <id> --name <n>
tasty agent barrier-await  --workspace-id <id> --name <n>
tasty agent barrier-state  --workspace-id <id> --name <n>
```

---

## 3. Semaphore (Phase 5.2)

N permit 까지 동시 점유. 같은 holder 가 같은 semaphore 를 다시 `acquire` 하면 idempotent 성공 (retry-safe). permit 회복은 그 holder 의 `release` 만.

### IPC

| method | 요약 |
|---|---|
| `semaphore_create` | `{ workspace_id, name, permits≥1 }` |
| `semaphore_acquire` | `{ ..., holder }` → `{ acquired, semaphore }`. 소진 시 `acquired:false` |
| `semaphore_release` | `{ ..., holder }` → `{ released, semaphore }`. 점유 holder 아니면 no-op |

### CLI

```
tasty agent semaphore-create  --workspace-id <id> --name <n> --permits <N>
tasty agent semaphore-acquire --workspace-id <id> --name <n> --holder <h>
tasty agent semaphore-release --workspace-id <id> --name <n> --holder <h>
```

---

## 4. Lease (Phase 5.3)

협조적(advisory) 자원 점유 마커 + TTL. OS 락이 아니라서 lease 를 무시한 외부 접근은 막지 못한다 (위반 감지 + 알림 수준).

- 영속 키: `tasty.agent.lease.<encoded-resource>` (memory 키 허용 문자만 — `[a-z0-9._-]`, 나머진 `_xx` hex escape; 원본 resource 는 JSON 본문에 그대로 저장하므로 디코딩 불필요)
- 모드 `fail` (기본) — 충돌 시 `-32009 lease_conflict`
- 모드 `block` — 충돌 시 `acquired:false` 반환 (예외 아님)
- 만료 lease 는 `list` / `acquire` 호출 시 lazy evict
- 같은 holder 재acquire 는 idempotent (TTL 재설정)

### IPC

| method | 요약 |
|---|---|
| `lease_acquire` | `{ ..., resource, holder, ttl_ms?, mode?∈{fail,block}=fail }` |
| `lease_release` | `{ ..., resource, holder }` — 점유 holder 아니면 no-op |
| `lease_list` | `{ workspace_id }` — 만료 자동 evict |

### CLI

```
tasty agent lease-acquire --workspace-id <id> --resource <r> --holder <h> [--ttl-ms <ms>] [--mode fail|block]
tasty agent lease-release --workspace-id <id> --resource <r> --holder <h>
tasty agent lease-list    --workspace-id <id>
```

---

## 5. Reducer (Phase 5.4)

N 개 task 결과를 단일 값으로 합성. 본 호출은 동기적 — 입력 task 가 아직 안 끝나면 `output` 은 `null` 로 들어간다 (완료 보장은 호출자 책임). `task_reduce` 는 단발 IPC; `TaskCommand::Reduce` 는 reducer 를 DAG 노드로 표현.

### 5 전략 (`strategy.kind`)

| 전략 | 동작 |
|---|---|
| `first_success` | 첫 `Succeeded` task 의 `output`. 성공한 입력 없으면 `-32602` |
| `all` | 모든 입력 `output` 을 순서대로 JSON 배열로 (상태 무관) |
| `merge_json` | 모든 입력 `output` (JSON object) left-to-right deep merge. non-object 거부. `null` 은 skip |
| `concat_text` | 모든 입력 `output` 텍스트 이어 붙임 (string 그대로, 다른 타입 JSON 직렬화) |
| `custom` | 호스트 shell (`sh -c` / Windows `cmd /C`) 로 명령 실행. stdin 에 `[output1, output2, ...]` JSON 배열, stdout 이 결과 (JSON parse 실패 시 string). exit ≠ 0 이면 `-32602` + stderr |

### IPC / CLI

```
agent.task_reduce { workspace_id, inputs: [TaskId], strategy: { kind, command? } } → { value }
```

```
tasty agent task-reduce --workspace-id <id> --inputs T1,T2,T3 \
    --strategy first_success|all|merge_json|concat_text|custom:<command>
```

---

## 6. Rate-limit (Phase 5.5)

(agent, metric) 쌍에 대한 token bucket. 보충률 = `limit / per_ms` tokens/ms, 상한 = `burst` (기본 = `limit`). 영속은 `Global` scope (agent 전역 — workspace 무관).

**`telemetry.cap` (관측 04) 과의 차이:**

| 시스템 | 의미 | 차단 시점 |
|---|---|---|
| `telemetry.cap` | 누적 임계 (예: `input_tokens` 총합 ≥ 100000) | 합산값이 임계 도달 |
| `agent.rate_limit` | 시간당 비율 (예: `ipc_calls` 100/분) | 윈도우 내 토큰 소진 |

본 단계는 CRUD + `try_consume` 만 노출 — IPC dispatcher 자동 평가 미들웨어는 후속 phase 에서 결합한다 (`telemetry.cap` 처럼 dispatcher 가 모든 plugin IPC 직전에 자동 검사). 현재는 호출자가 `try_consume` 을 명시적으로 호출.

### IPC

| method | 요약 |
|---|---|
| `rate_limit_set` | `{ agent, metric, limit≥1, per_ms≥1, burst? }` upsert. 동일 (agent, metric) 은 같은 id 유지 + 버킷 reset |
| `rate_limit_list` | refill 적용 후 전체 |
| `rate_limit_remove` | `{ id }` |
| `rate_limit_status` | `{ agent?, metric? }` 필터 |

### CLI

```
tasty agent rate-limit-set    --agent <id> --metric <name> --limit <n> --per-ms <ms> [--burst <n>]
tasty agent rate-limit-list
tasty agent rate-limit-remove --id <rate-limit-id>
tasty agent rate-limit-status [--agent <id>] [--metric <name>]
```

---

## 에러 코드

| 코드 | 의미 |
|---|---|
| `-32004` | task / resource not found |
| `-32008` | task already terminal (cancel 등 거부) |
| `-32009` | lease conflict (mode=fail) |
| `-32602` | invalid_params (사이클, 미존재 dep, 잘못된 strategy, custom shell 비정상 종료 등) |
| `-32603` | internal (memory store 등) |

## 다음 단계

- Phase 6 (예정): Plugin permission 모델 + capability 정제 — `process.spawn` capability 가 추가되면 reducer `custom` 은 추가 권한을 요구하게 된다.
- Scheduler 도입 시 `*_await` 가 blocking + wakeup 으로 분기.
- Rate-limit 의 IPC dispatcher 자동 평가 미들웨어 결합 (`telemetry.cap` 처럼).
