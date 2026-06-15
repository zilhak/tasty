# 공유 컨텍스트 (Phase 7)

- **Status**: Implemented

여러 에이전트가 같은 워크스페이스 상태를 보고/갱신하기 위한 표면 3 종. 모두 일반 `memory.*` 영역 위의 얇은 래퍼 — 별도 저장소가 아니라 키 컨벤션 + 검증을 묶어둔 것이다. 권한은 `memory.write` / `memory.read` 를 그대로 재사용하고, 변경은 일반 `memory.changed` 이벤트로 발화된다. 신규 에러 코드 `-32009 already_exists`.

상세 가이드: [agent-guide/blackboard.md](agent-guide/blackboard.md), [agent-guide/plan.md](agent-guide/plan.md), [agent-guide/cache.md](agent-guide/cache.md).

### Blackboard (Phase 7.1)

워크스페이스 단위 명명된 키-값 컬렉션. 키 컨벤션 `tasty.bb.<name>._meta` + `tasty.bb.<name>.fields.<field>`. `_meta.schema` 는 임의 JSON 으로 보관 (호스트는 검증하지 않음 — 호출자가 직접).

- IPC 9 종: `memory.bb_{create,put,get,get_all,get_meta,delete_field,delete,list,exists}`. 모두 `workspace_id` 필수.
- 이름 규칙: name/field 1..=64 자 `[a-z0-9_-]+`. `bb_put` 은 `_meta` 가 없으면 `-32004`.
- owner 규칙: 일반 memory 와 동일 — caller 가 만든 entry 만 caller 가 수정, `_host` 는 root.
- CLI: `tasty memory bb {create,put,get,get-all,get-meta,delete-field,delete,list,exists}`.

### Plan (Phase 7.2)

워크스페이스 단위 선언적 work breakdown. 한 plan = `tasty.plan.<plan_id>` 단일 JSON entry (step 1 개 갱신도 plan 전체 JSON put 1 회).

- IPC 7 종: `memory.plan_{create,get,list,delete,add_step,remove_step,update_step}`.
- `PlanStepState` 5 종: `pending` / `in_progress` / `completed` / `failed` / `skipped`.
- 검증: flat step 수 ≤ 256, step id 중복 금지, `depends_on` ref 유효성 + 자기 의존/사이클 금지 (DFS).
- `update_step` 의 notes 3 분기: `notes` (set) / `clear_notes:true` (해제) / 둘 다 없음 (유지).
- CLI: `tasty memory plan {create,get,list,delete,add-step,remove-step,update-step}`.
- JSON Schema: [agent-guide/plan.schema.json](agent-guide/plan.schema.json).

**`agent.task_*` 와의 구분**: `agent.task_*` (Phase 5.1) 는 실행기 (스케줄러가 `ready → running → done` 진행), `memory.plan_*` 는 상태 기록 — 호출자가 명시적으로 update-step 호출. 같은 워크스페이스에서 둘을 함께 써도 무방.

### Cache (Phase 7.3)

워크스페이스 단위 TTL 캐시. 키 prefix `tasty.cache.<key>` + 필수 양수 `ttl_secs` 규약.

- IPC 5 종: `memory.cache_{put,get,invalidate,clear,list}`.
- `cache_get` 의 만료/미존재는 둘 다 `null` — 호출자가 구분 불필요.
- `cache_invalidate` 는 idempotent (없어도 성공). `cache_clear` 는 caller 가 수정권 있는 entry 만.
- CLI: `tasty memory cache {put,get,invalidate,clear,list}`.

### Snapshot / Restore (Phase 7.4)

bb 의 한 시점을 통째로 캡처해 복원. 키 컨벤션 `tasty.bb.<name>.snapshots.<sid>` (sid 1..=64 자 `[a-z0-9_-]+`).

- IPC 5 종: `memory.bb_snapshot{,_get,_list,_delete,_restore}`.
- 페이로드: `BlackboardSnapshot { bb_name, snapshot_id, taken_at, taken_by, meta?, fields[] }`. `fields[].payload` 는 `content_type` 별로 평탄화 (text → string / json → value / binary → base64 string) — `MemoryValue` 의 internally-tagged enum 이 `serde_json::to_value` 경로에서 깨지는 문제를 피함.
- restore 동작: 현재 field 를 모두 지운 뒤 snapshot 의 field 를 **현재 caller** 가 owner 로 다시 put. 원래 owner 정보는 복원되지 않는다. bb 가 (수동 삭제 등으로) 사라졌으면 snapshot 의 `meta` 로 재생성.
- `bb_delete` 는 fields + snapshots + meta 모두 삭제. snapshot 보존이 필요하면 외부에서 `bb_snapshot_get` 으로 복사.
- CLI: `tasty memory bb {snapshot,snapshot-get,snapshot-list,snapshot-delete,snapshot-restore}`.
