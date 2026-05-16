# Cache (`memory.cache_*`)

워크스페이스 단위 **TTL 기반 키-값 캐시**. 일반 `memory.*` 위에 prefix + 필수 TTL 규약만 입힌 얇은 래퍼다.

## 개요

- **scope**: 항상 `workspace:<id>`.
- **owner**: 일반 memory 규칙과 동일 — caller 가 만든 entry 만 caller 가 수정·삭제 (`_host` 는 root).
- **권한**: 쓰기 → `memory.write`, 읽기 → `memory.read`.
- **만료**: 모든 entry 는 양수 `ttl_secs` 를 가져야 한다 (`0` 은 거부). expiry 가 지나면 `cache_get` / `cache_list` 응답에서 자동 제외 — 디스크 회수는 `tasty memory gc` 또는 surface/workspace 정리 시점에 발생.

## 키 컨벤션

모든 entry 는 다음 키 한 종류에 매핑된다:

```
tasty.cache.<key>
```

- `<key>` 는 호출자가 의미 있는 식별자로 직접 정한다 (예: 입력 해시).
- 검증: 1..=200 자, `[a-z0-9._-]+`. 점(.) 은 캐시 키에서 허용된다 (memory 키 규칙을 그대로 따른다).

`memory.list --prefix tasty.cache.` 로 raw entry 를 볼 수 있다 — cache 는 별도 테이블이 아니다.

## IPC 메서드

권한 표기: **W** = `memory.write`, **R** = `memory.read`.

| 메서드 | 권한 | 파라미터 | 응답 |
|---|---|---|---|
| `memory.cache_put`        | W | `{ workspace_id, key, value 또는 value_b64, content_type?, ttl_secs }` | `{ ok, version }` |
| `memory.cache_get`        | R | `{ workspace_id, key }` | entry 객체 또는 `null` (만료/미존재 동일) |
| `memory.cache_invalidate` | W | `{ workspace_id, key }` | `{ ok }` (없어도 성공 — idempotent) |
| `memory.cache_clear`      | W | `{ workspace_id }` | `{ ok, removed }` (caller 가 수정권 있는 entry 만) |
| `memory.cache_list`       | R | `{ workspace_id }` | `{ keys, count }` (prefix 제거된 형태, 정렬) |

세부사항:

- `cache_put` 의 `ttl_secs` 는 필수 양수. 미지정/0 은 `-32602`.
- `cache_get` 은 만료 entry 도 `null` 로 응답 — 호출자가 "miss" 와 "expired" 를 구분할 필요가 없다.
- `cache_invalidate` 는 entry 가 없어도 성공 (`NotFound` 를 흡수).
- `cache_clear` 는 owner 가 수정할 수 없는 entry (예: 다른 plugin 이 만든 캐시) 가 있으면 `-32006 owned_by_other` 로 중단 — 자기 entry 만 정리하고 싶다면 `cache_list` + 개별 `cache_invalidate` 조합을 쓴다.

## CLI

```bash
# 저장 (TTL 필수)
tasty memory cache put --workspace 7 --key sha256.abcd1234 --value "..." --ttl 3600
tasty memory cache put --workspace 7 --key result.json --value '{"answer":42}' --ttl 600
tasty memory cache put --workspace 7 --key blob --value-b64 AAEC --content-type application/octet-stream --ttl 60
tasty memory cache put --workspace 7 --key body --value @/tmp/payload.txt --ttl 1800

# 조회 / 무효화
tasty memory cache get        --workspace 7 --key sha256.abcd1234
tasty memory cache invalidate --workspace 7 --key sha256.abcd1234
tasty memory cache list       --workspace 7
tasty memory cache clear      --workspace 7
```

## 에러 코드

| code | 의미 |
|---|---|
| `-32602` | invalid params (key 검증 실패, `ttl_secs` 0 / 누락) |
| `-32006` | `owned_by_other` (다른 owner 의 entry 를 `cache_invalidate` / `cache_clear` 시도) |
| `-32007` | `quota_exceeded` |

`cache_get` 의 만료/미존재는 에러가 아니라 `null` 결과다.

## 시나리오

### 1) LLM 응답 캐시

```bash
KEY="sha256.$(echo "$PROMPT" | sha256sum | cut -d' ' -f1)"
HIT=$(tasty memory cache get --workspace 7 --key "$KEY")
if [ "$HIT" = "null" ]; then
    RESPONSE=$(call_llm "$PROMPT")
    tasty memory cache put --workspace 7 --key "$KEY" --value "$RESPONSE" --ttl 86400
else
    echo "$HIT" | jq -r .value
fi
```

### 2) 짧은 TTL 로 hot lookup 재사용

```bash
tasty memory cache put --workspace 7 --key proj.cwd --value "/repo/foo" --ttl 60
# 다른 에이전트가 1 분 안에 같은 key 조회 → hit
```

### 3) Plugin 매니페스트

쓰기/읽기 모두 같은 권한 토큰을 그대로 쓴다 — 별도 cache 권한은 없다:

```toml
[[contributes.permissions]]
permission = "memory.write"

[[contributes.permissions]]
permission = "memory.read"
```

## TTL / GC 관계

- read 경로에서 만료 entry 가 자동으로 가려지므로, 사용자에게 보이는 동작은 디스크 회수 시점과 무관하다.
- 디스크 + quota 회수가 필요하면 `tasty memory gc` (Local-only) 를 호출한다. surface/workspace 가 닫힐 때 해당 scope 의 entry 가 자동 정리된다.
- `expires_at` 의 절대 시각이 필요하면 일반 `memory.put --expires-at <unix_ms>` 를 직접 사용한다 — cache 는 항상 상대 TTL (`ttl_secs`) 만 받는다.

## 관련 문서

- 일반 memory API: [`api-reference.md`](api-reference.md) §"에이전트 메모리"
- Blackboard: [`blackboard.md`](blackboard.md)
- Plan: [`plan.md`](plan.md)
