# Plan (`memory.plan_*`)

워크스페이스 단위 **선언적 work breakdown**. 한 plan = 의존성을 가진 step 목록 + 각 step 의 상태. 호스트는 plan 의 상태만 보관할 뿐, **스케줄러도 실행기도 아니다**. step state 전이는 호출자 (사람 / 에이전트) 가 직접 한다.

## `agent.task_*` 와의 차이

| | `memory.plan_*` (Phase 7.2) | `agent.task_*` (Phase 5.1) |
|---|---|---|
| 성격 | 선언적 — "할 일 트리" 의 기록 | 실행기 — `ready → running → done` 을 호스트가 진행 |
| 상태 변경 | 호출자가 명시 호출 (`plan_update_step`) | dep 충족 시 자동으로 `ready` 진입, 호스트가 실행 트리거 |
| primitive | step (1 종) | task + barrier + semaphore + lease + reducer + rate-limit (6 종) |
| 영속 | `tasty.plan.<plan_id>` 단일 JSON entry | `tasty.agent.task.<id>` 등 다수 키 |
| 권한 | `memory.write` / `memory.read` | `agent` (`AgentManage`) |

같은 워크스페이스에서 둘을 같이 써도 무방하다 — plan 은 사람·에이전트가 협의하는 "회의록", task 는 자동 실행 큐. 서로 연결할지는 호출자 자유 (예: plan step 의 `notes` 에 `agent.task_get` id 를 적어둔다).

## 영속 모델

한 plan 은 `tasty.plan.<plan_id>` 키에 **단일 JSON value** 로 직렬화된다. step 한 개를 갱신하는 호출도 결국 plan 전체 JSON 의 `memory.put` 한 번으로 변환된다.

- **scope**: `workspace:<id>`.
- **owner**: 생성한 caller. 갱신·삭제는 owner 본인 또는 `_host`.
- **검증** (모든 put 시점):
  - `id` / `step.id`: 1..=64 자, `[a-z0-9_-]+`.
  - 제목: 1..=256 자.
  - notes: ≤2048 자.
  - flat step 수: ≤ 256.
  - step id 중복 금지.
  - `depends_on` 의 id 는 같은 plan 의 다른 step 을 가리켜야 하고, 자기 자신 의존 금지, 사이클 금지 (DFS 로 검출).

위 invariant 위반은 `-32602 invalid params`.

## JSON 스키마

전체 Plan 형태 (한 row 의 value):

```json
{
  "id": "release-1.0",
  "title": "1.0 release prep",
  "created_at": 1715800000000,
  "created_by": "_host",
  "updated_at": 1715800050000,
  "steps": [
    {
      "id": "qa",
      "title": "QA pass",
      "state": "pending",
      "depends_on": [],
      "notes": null
    },
    {
      "id": "notes",
      "title": "Write release notes",
      "state": "in_progress",
      "depends_on": ["qa"],
      "notes": "draft in /tmp/notes.md"
    }
  ]
}
```

`PlanStepState` 는 다음 5 종 중 하나 (`snake_case` 직렬화):

| state | 의미 |
|---|---|
| `pending`     | 시작 전 (기본값) |
| `in_progress` | 진행 중 |
| `completed`   | 완료 |
| `failed`      | 실패 |
| `skipped`     | 건너뜀 |

`state` / `depends_on` / `notes` 는 직렬화 시 default 값 (`pending`, `[]`, `null`) 이면 생략될 수 있다 — 디시리얼라이즈는 누락 시 default 로 채운다.

기계가 읽을 수 있는 JSON Schema 는 [`plan.schema.json`](plan.schema.json) 에 있다.

## IPC 메서드

권한 표기: **W** = `memory.write`, **R** = `memory.read`.

| 메서드 | 권한 | 파라미터 | 응답 |
|---|---|---|---|
| `memory.plan_create`      | W | `{ workspace_id, plan_id, title, steps? }` | `{ ok, version }` (= 1) |
| `memory.plan_get`         | R | `{ workspace_id, plan_id }` | `Plan` JSON 또는 `null` |
| `memory.plan_list`        | R | `{ workspace_id }` | `{ plans, count }` (id 목록) |
| `memory.plan_delete`      | W | `{ workspace_id, plan_id }` | `{ ok }` |
| `memory.plan_add_step`    | W | `{ workspace_id, plan_id, step, position?, cas? }` | `{ ok, version }` |
| `memory.plan_remove_step` | W | `{ workspace_id, plan_id, step_id, cas? }` | `{ ok, version }` |
| `memory.plan_update_step` | W | `{ workspace_id, plan_id, step_id, state?, notes?, clear_notes?, cas? }` | `{ ok, version }` |

세부사항:

- `plan_create` 의 `steps` 는 step 객체 배열. 미지정 시 빈 plan.
- `plan_add_step` 의 `position` 미지정 시 끝에 append. 표시 순서는 배열 순서, 실행 순서는 `depends_on` 으로 표현한다.
- `plan_remove_step` 은 다른 step 이 `depends_on` 으로 참조 중이면 거부.
- `plan_update_step` 의 notes 인자 3 분기:
  - `notes` 만 보내면 set
  - `clear_notes: true` 면 None 으로 해제 (`notes` 와 동시 사용 불가)
  - 둘 다 없으면 기존 notes 유지
- CAS 는 plan entry 의 `version` 기준. step 단건 version 은 없다 (plan 전체가 하나의 row).

## CLI

```bash
# 생성 / 조회 / 삭제
tasty memory plan create --workspace 7 --plan-id release-1.0 --title "1.0 release prep" \
    --steps '[{"id":"qa","title":"QA pass"},{"id":"notes","title":"Release notes","depends_on":["qa"]}]'
tasty memory plan list   --workspace 7
tasty memory plan get    --workspace 7 --plan-id release-1.0
tasty memory plan delete --workspace 7 --plan-id release-1.0

# Step 조작
tasty memory plan add-step    --workspace 7 --plan-id release-1.0 \
    --step '{"id":"announce","title":"Send announcement","depends_on":["notes"]}' \
    --cas 3
tasty memory plan add-step    --workspace 7 --plan-id release-1.0 \
    --step '{"id":"first","title":"kick off"}' --position 0
tasty memory plan remove-step --workspace 7 --plan-id release-1.0 --step-id announce --cas 5

# 상태 / notes 갱신
tasty memory plan update-step --workspace 7 --plan-id release-1.0 \
    --step-id qa --state in_progress
tasty memory plan update-step --workspace 7 --plan-id release-1.0 \
    --step-id notes --notes "draft on /tmp/notes.md"
tasty memory plan update-step --workspace 7 --plan-id release-1.0 \
    --step-id notes --clear-notes
tasty memory plan update-step --workspace 7 --plan-id release-1.0 \
    --step-id qa --state completed --cas 6
```

## 에러 코드

`memory.*` 의 코드를 그대로 사용한다:

| code | 의미 |
|---|---|
| `-32602` | invalid params (step id 중복, 사이클, depends_on 미존재, 길이 초과 등) |
| `-32004` | `not_found` (plan/step 미존재) |
| `-32005` | `cas_conflict` |
| `-32006` | `owned_by_other` |
| `-32007` | `quota_exceeded` |
| `-32009` | `already_exists` (`plan_create` 중복 id) |

## 시나리오

### 1) 회의에서 합의된 work breakdown 기록

```bash
tasty memory plan create --workspace 7 --plan-id phase8 --title "Phase 8 work" \
    --steps '[
        {"id":"design","title":"design doc"},
        {"id":"prototype","title":"prototype","depends_on":["design"]},
        {"id":"review","title":"design review","depends_on":["design"]},
        {"id":"impl","title":"implementation","depends_on":["prototype","review"]}
    ]'
```

### 2) Step state 진행 + downstream 확인

```bash
tasty memory plan update-step --workspace 7 --plan-id phase8 --step-id design --state completed
# design 이 completed 가 되면 prototype/review 두 step 이 unblock 된다 — plan 은 자동 트리거하지 않으니
# 호출자가 plan_get 으로 보고 다음 작업을 골라 또 update-step 한다.
tasty memory plan get --workspace 7 --plan-id phase8
```

### 3) Plugin 에서 plan 갱신 구독

`memory.changed` 이벤트가 `key=tasty.plan.<plan_id>` 로 도착한다. plan 의 변경 단위가 step 한 개라도 entry 가 하나이므로 한 변경 = 한 이벤트.

```toml
[[contributes.permissions]]
permission = "memory.read"

[[contributes.event_subscribe]]
event = "memory.changed"
```

## 관련 문서

- 일반 memory API: [`api-reference.md`](api-reference.md) §"에이전트 메모리"
- 실행기가 필요한 작업 그래프: [`agent.md`](agent.md) §"Task DAG"
- Blackboard: [`blackboard.md`](blackboard.md)
- 이벤트 카탈로그: [`event-catalog.md`](event-catalog.md) §"Memory"
- 기계가 읽을 JSON Schema: [`plan.schema.json`](plan.schema.json)
