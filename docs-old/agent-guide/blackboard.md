# Blackboard (`memory.bb_*`)

워크스페이스 단위로 여러 에이전트가 공유하는 **명명된 키-값 컬렉션**. 일반 `memory.*` API 위의 얇은 래퍼 — 다른 저장소가 아니라 키 컨벤션 + 스키마 슬롯 + snapshot 기능을 묶어둔 것이다.

## 개요

- **scope**: 항상 `workspace:<id>`. 워크스페이스 경계를 넘는 bb 는 만들 수 없다.
- **owner 규칙**: `_meta` 와 각 field 는 일반 memory entry 와 동일한 owner 강제를 받는다. caller 가 만든 entry 만 caller 가 갱신·삭제할 수 있고, `_host` (CLI) 는 root.
- **권한**: 쓰기 메서드 → `memory.write`, 읽기 메서드 → `memory.read`. snapshot 도 동일 분류.
- **schema**: `bb_create` 시 임의 JSON 을 `schema` 로 전달하면 `_meta.schema` 에 보관된다. 호스트는 검증하지 않는다 — 호출자가 직접 검사한다.
- **이벤트**: 모든 변경은 일반 `memory.changed` 이벤트로 발화된다 (`scope=workspace:<id>`, key=해당 entry 의 raw key). 별도 watch IPC 는 없다.

## 키 컨벤션

본 모듈이 메모리에 쓰는 row 는 다음 3 종류뿐이다:

| 키 | 내용 |
|---|---|
| `tasty.bb.<name>._meta`              | JSON `BlackboardMeta { name, schema?, created_at, created_by }` |
| `tasty.bb.<name>.fields.<field>`     | 사용자 값 (text/json/binary 자유) |
| `tasty.bb.<name>.snapshots.<sid>`    | JSON `BlackboardSnapshot` (한 시점의 fields 캡처) |

이름 규칙:

- `name` / `field` / `snapshot_id`: 1..=64 자, `[a-z0-9_-]+`. 점(.) 은 컨벤션 구분자라 금지.
- 한 워크스페이스 안에서 같은 `name` 의 bb 는 1 개뿐 — 중복 `bb_create` 는 `-32009 already_exists`.

`memory.list --prefix tasty.bb.` 로 모든 raw row 를 그대로 볼 수 있다 — bb 는 별도 테이블이 아니다.

## IPC 메서드

권한 표기: **W** = `memory.write`, **R** = `memory.read`.

### Base (Phase 7.1)

| 메서드 | 권한 | 파라미터 | 응답 |
|---|---|---|---|
| `memory.bb_create`       | W | `{ workspace_id, name, schema? }` | `{ ok, version }` (= 1) |
| `memory.bb_put`          | W | `{ workspace_id, name, field, value 또는 value_b64, content_type?, cas? }` | `{ ok, version }` |
| `memory.bb_get`          | R | `{ workspace_id, name, field }` | entry 객체 또는 `null` |
| `memory.bb_get_all`      | R | `{ workspace_id, name }` | `{ entries, count }` (fields 만, `_meta` 제외) |
| `memory.bb_get_meta`     | R | `{ workspace_id, name }` | `_meta` entry 또는 `null` |
| `memory.bb_delete_field` | W | `{ workspace_id, name, field, cas? }` | `{ ok }` |
| `memory.bb_delete`       | W | `{ workspace_id, name }` | `{ ok, removed }` (fields + snapshots + meta 합산) |
| `memory.bb_list`         | R | `{ workspace_id }` | `{ names, count }` |
| `memory.bb_exists`       | R | `{ workspace_id, name }` | `{ exists }` |

`bb_put` 은 `_meta` 가 없으면 `-32004 not_found` — 반드시 `bb_create` 가 먼저 와야 한다.

### Snapshot (Phase 7.4)

| 메서드 | 권한 | 파라미터 | 응답 |
|---|---|---|---|
| `memory.bb_snapshot`         | W | `{ workspace_id, name, snapshot_id }` | `{ ok, version }` |
| `memory.bb_snapshot_get`     | R | `{ workspace_id, name, snapshot_id }` | `BlackboardSnapshot` JSON 또는 `null` |
| `memory.bb_snapshot_list`    | R | `{ workspace_id, name }` | `{ snapshot_ids, count }` (정렬) |
| `memory.bb_snapshot_delete`  | W | `{ workspace_id, name, snapshot_id }` | `{ ok }` |
| `memory.bb_snapshot_restore` | W | `{ workspace_id, name, snapshot_id }` | `{ ok, restored }` (복원된 field 수) |

#### `BlackboardSnapshot` JSON 형태

```json
{
  "bb_name": "review",
  "snapshot_id": "v1",
  "taken_at": 1715800001234,
  "taken_by": "_host",
  "meta": { "name": "review", "schema": null, "created_at": 1715800000000, "created_by": "_host" },
  "fields": [
    { "field": "title",  "content_type": "text/plain",        "payload": "..." },
    { "field": "data",   "content_type": "application/json",  "payload": { "ok": true } },
    { "field": "blob",   "content_type": "application/octet-stream", "payload": "AAEC..." }
  ]
}
```

`fields[].payload` 는 `content_type` 별로 평탄화된다:

- `text/plain` → JSON string
- `application/json` → 임의 JSON value
- `application/octet-stream` → base64 string (디코드 후 binary entry 로 복원)

#### restore 동작

1. 현재 bb 의 모든 field 를 삭제 — caller 가 owner 이거나 `_host` 일 때만 성공. 다른 owner 의 field 가 있으면 `-32006 owned_by_other`.
2. bb 가 (수동 삭제 등으로) 사라졌으면 snapshot 의 `meta` 로 `_meta` 를 다시 만든다.
3. snapshot 의 각 field 를 **현재 caller** 가 owner 로 다시 `put`. 원래 owner 정보는 복원되지 않는다.

snapshot 자체 entry 는 그대로 남아 반복 restore 가 가능하다. `bb_delete` 는 모든 snapshot 도 함께 삭제하므로, 보존이 필요하면 외부에서 `bb_snapshot_get` 으로 복사해두어야 한다.

## CLI

모든 명령은 `--workspace <id>` 필수.

```bash
# 생성 / 존재 확인
tasty memory bb create   --workspace 7 --name review [--schema '{"required":["title"]}']
tasty memory bb list     --workspace 7
tasty memory bb exists   --workspace 7 --name review
tasty memory bb get-meta --workspace 7 --name review

# 필드 쓰기 / 읽기 / 삭제 (값 형식은 일반 memory 와 동일)
tasty memory bb put          --workspace 7 --name review --field title --value "first pass"
tasty memory bb put          --workspace 7 --name review --field data  --value '{"ok":true}'
tasty memory bb put          --workspace 7 --name review --field buf   --value @/tmp/buf.txt
tasty memory bb put          --workspace 7 --name review --field bin   --value-b64 AAEC --content-type application/octet-stream
tasty memory bb get          --workspace 7 --name review --field title
tasty memory bb get-all      --workspace 7 --name review
tasty memory bb delete-field --workspace 7 --name review --field tmp --cas 3
tasty memory bb delete       --workspace 7 --name review

# 스냅샷
tasty memory bb snapshot         --workspace 7 --name review --snapshot-id v1
tasty memory bb snapshot-list    --workspace 7 --name review
tasty memory bb snapshot-get     --workspace 7 --name review --snapshot-id v1
tasty memory bb snapshot-restore --workspace 7 --name review --snapshot-id v1
tasty memory bb snapshot-delete  --workspace 7 --name review --snapshot-id v1
```

## 동시성 / CAS

`bb_put` 과 `bb_delete_field` 는 일반 `memory.put` / `memory.delete` 와 같은 CAS 를 지원한다. 두 에이전트가 같은 field 를 동시에 쓰는 경우:

```
A: bb_get → version=5
B: bb_get → version=5
A: bb_put --cas 5 → ok, version=6
B: bb_put --cas 5 → -32005 cas_conflict (B 가 다시 read 후 retry)
```

CAS 미사용 시 last-write-wins. 다중 에이전트 협업에서는 CAS 사용을 권장.

## 에러 코드

`memory.*` 의 코드를 그대로 사용한다:

| code | 의미 |
|---|---|
| `-32602` | invalid params (name/field 검증 실패, missing key) |
| `-32004` | `not_found` (bb 미존재 상태에서 `bb_put`, snapshot 미존재 restore 등) |
| `-32005` | `cas_conflict` |
| `-32006` | `owned_by_other` (다른 plugin/agent 가 만든 field 갱신 시도) |
| `-32007` | `quota_exceeded` |
| `-32009` | `already_exists` (`bb_create` 중복, `bb_snapshot` 동일 sid 재생성) |

## 시나리오

### 1) 리뷰 보드를 두 에이전트가 채우기

A 가 `_meta` 와 `title` field 를 만들고, B 는 `comments[]` JSON field 를 채운다.

```bash
# A
tasty memory bb create --workspace 7 --name review --schema '{"fields":["title","comments"]}'
tasty memory bb put    --workspace 7 --name review --field title --value "PR #42 review"

# B
tasty memory bb put    --workspace 7 --name review --field comments --value '[{"by":"B","text":"LGTM"}]'

# 양쪽 다 읽음
tasty memory bb get-all --workspace 7 --name review
```

### 2) 위험한 변경 전 snapshot → restore

```bash
tasty memory bb snapshot --workspace 7 --name review --snapshot-id pre-rewrite

# 대량 변경 ...
tasty memory bb put --workspace 7 --name review --field title --value "rewritten"
tasty memory bb delete-field --workspace 7 --name review --field comments

# 마음에 안 들면 원복
tasty memory bb snapshot-restore --workspace 7 --name review --snapshot-id pre-rewrite
```

### 3) Plugin 에서 변경 구독

매니페스트:

```toml
[[contributes.permissions]]
permission = "memory.read"

[[contributes.event_subscribe]]
event = "memory.changed"
```

`memory.changed` payload 의 `scope`/`key` 를 보고 `tasty.bb.<name>.fields.<field>` prefix 로 필터하면 bb 변경을 흘려보낼 수 있다. 별도 bb-전용 이벤트는 없다.

## 관련 문서

- 일반 memory API: [`api-reference.md`](api-reference.md) §"에이전트 메모리"
- Plan: [`plan.md`](plan.md)
- Cache: [`cache.md`](cache.md)
- 이벤트 카탈로그: [`event-catalog.md`](event-catalog.md) §"Memory"
