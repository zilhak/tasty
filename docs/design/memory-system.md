# 에이전트 메모리 시스템 (Agent Memory System)

`~/.tasty/memory.db` 에 저장되는 영속 키-값 스토어. AI 에이전트와 plugin이 작업 도중
누적·검색·공유하는 데이터의 backing store다. SQLite (WAL) 단일 파일, 본 바이너리
`OnceLock<Mutex<MemoryStore>>` 싱글톤으로 동기 접근.

상세 위치/계층 규칙은 [storage-system.md](storage-system.md) 의 `~/.tasty/` 표 참조.
본 문서는 **메모리 영역의 가시성·소유권 모델**을 정의한다.

## 원칙: 어디까지 책임지는가

Tasty memory 는 **에이전트·plugin 이 작업 도중 누적·검색·공유하는 데이터의 1차 저장소** 다. 작은 키-값만 받는 협소한 곳이 아니라, **합리적인 한도 안에서는 범용적으로** 쓸 수 있다 — 토큰 같은 수 KB 부터 캐시된 JSON 응답이나 누적된 작업 상태 같은 수백 KB ~ 1 MiB 단위까지 자연스럽게 다룬다.

하지만 **"어떤 크기든 받아주는 만능 저장소" 는 아니다.** 일정 크기를 넘어가는 데이터는 memory 가 책임지는 영역이 아니다.

### "지원 한계를 넘는 데이터는 외부 파일 + memory 에 링크" 원칙

Plugin 이 cap 을 넘는 데이터를 다뤄야 할 때, memory entry 크기를 키우거나 cap 설정을 올리는 방향이 아니라 **개별 파일을 만들고 그 경로를 memory entry 로 저장**하는 방향으로 설계해야 한다.

```text
# 안티패턴: 큰 blob 을 memory 에 직접
memory.put(scope="workspace:1", key="screenshot.png.b64", value=<5MB base64>)  // ❌ ValueTooLarge

# 권장: filesystem + reference
fs.write("~/.tasty/plugins/com.foo.bar/data/screenshot-2024.png", bytes)
memory.put(scope="workspace:1", key="screenshot.path",
           value="~/.tasty/plugins/com.foo.bar/data/screenshot-2024.png")
```

이 원칙이 의도하는 것:

- **memory.db 비대 방지.** SQLite 단일 파일에 GB 단위 blob 을 누적시키면 백업·동기화·복구가 모두 무거워진다. Tasty 가 모든 plugin 의 모든 데이터를 끌어안는 방향은 아니다.
- **명시적 lifecycle.** filesystem 파일은 plugin uninstall/cleanup 시 plugin 자기 데이터 폴더 통째 삭제로 정리된다. memory blob 으로 누적된 큰 데이터는 추적이 더 까다롭다.
- **OS 도구와의 호환성.** 큰 파일은 `du` / `find` / `rsync` / OS 백업 시스템과 자연스럽게 동작한다.

용량 cap (단일 entry 1 MiB, plugin secret 10 MiB, regular 전체 1 GiB) 은 이 원칙의 **권장 한계선** 이다. plugin 이 cap 에 부딪쳤을 때 first response 는 "config 올려달라" 가 아니라 **"이만큼 큰 데이터가 정말 memory 에 들어가야 하는 데이터인가, 파일로 분리할 수 있는가" 를 먼저 묻는 것** 이다. 정당한 이유가 있으면 config 로 올린다.

### 책임지는 것 / 책임지지 않는 것

| 책임진다 | 책임지지 않는다 |
|---|---|
| 작업 상태, 진행 메타데이터 | Plugin 의 큰 binary asset (이미지, 모델 weight, 미디어) |
| 토큰, 설정, 세션 정보 | Plugin 의 영구 로그 (별도 logging 시스템) |
| 캐시된 응답, 중간 결과 (cap 안) | Plugin 간 임의 크기 데이터 교환 (filesystem 참조 또는 `tasty-shm`) |
| Plugin 간 가벼운 공유 데이터 (regular) | 모든 plugin 데이터의 single source of truth (plugin 자기 데이터 폴더가 1차 권한자) |

## 두 계층: regular / secret

메모리는 의미적으로 **완전히 분리된 두 영역**으로 나뉜다. 같은 `memory.db` 파일 안에 살지만 테이블이 다르고, IPC 메서드 네임스페이스가 다르고, 가시성 규칙이 다르다.

| 영역 | IPC 네임스페이스 | 권한 토큰 | 가시성 | 쓰기 권한 |
|------|------------------|-----------|--------|-----------|
| Regular | `memory.*` | `memory.read` / `memory.write` | **모든 plugin이 모든 entry 읽기** | **owner 본인만** (또는 host) |
| Secret | `memory.secret.*` | `memory.secret` | **각 plugin은 자기 영역만 본다** | 각 plugin은 자기 영역만 |

두 영역은 같은 `Scope` enum(`global` / `account:<u>` / `window:<id>` / `workspace:<id>` / `surface:<id>`) 과 동일한 키 규칙(1..=256자 `[a-z0-9._-]+`) 을 공유한다. 차이는 오로지 **`owner` 라는 숨겨진 차원** 이다.

## owner: 숨겨진, 호스트 전용 차원

`owner` 는 모든 entry 에 항상 붙는 키지만 **plugin 에게는 존재 자체가 보이지 않는다**.

- Plugin 은 `owner` 를 **인자로 넘길 수 없다**. IPC 메서드 schema 에 `owner` 필드가 없다.
- Plugin 은 응답에서 자기 owner 를 보지 못한다 (secret 영역). Regular 영역은 응답에 owner 가 포함되지만, 이는 "누가 만든 건지" 의 정보일 뿐 plugin 이 그 값을 조작·지정할 수 있다는 뜻이 아니다.
- `owner` 는 **호스트가 caller 로부터 자동 도출**한다. `CallerContext` 만이 owner 를 결정한다.

```text
CallerContext::Plugin(plugin_id)  →  owner = plugin_id   (예: "com.tasty.claude")
CallerContext::Local              →  owner = "_host"     (CLI · 사용자 · 데이터 정리)
```

`_host` 의 underscore prefix 는 plugin id 의 reverse-DNS 규칙과 충돌하지 않도록 의도된 sentinel 이다. plugin 이 `_host` 라는 id 를 가질 수 없다.

### Local caller 는 root

`_host` 권한은 모든 entry 의 owner check 를 통과한다. CLI 와 사용자 행동은 "tasty 본체의 동작 / 사용자의 데이터 정리 규칙" 을 대표하므로 임의 entry 를 수정·삭제할 수 있다. 이건 우회로가 아니라 **설계상의 root 권한** 이다 — plugin 영역에 사용자가 손을 댈 수 없다면 plugin 이 만든 잘못된 데이터를 정리할 수 없다.

## Regular memory: 공유 네임스페이스 + owner enforcement

### 데이터 모델

```text
PK: (scope, key)
columns: value, content_type, created_at, updated_at, expires_at, version, owner
```

`(scope, key)` 가 **전역 unique** 다. owner 가 다른 plugin 두 개가 `(workspace:1, "task.plan")` 에 동시에 entry 를 둘 수 없다 — 먼저 쓴 쪽이 owner 가 되고, 다른 plugin 은 그 entry 를 갱신·삭제할 수 없다.

### 가시성

- **모든 read 메서드 (`get` / `list` / `exists` / `count` / `scopes` / `stats`)**: caller 와 무관하게 전체 영역을 본다.
- 응답 entry 에 `owner` 필드가 포함된다. 에이전트가 "이 데이터를 누가 만들었나" 를 알 수 있어야 plugin 간 충돌·신뢰 판단이 가능하기 때문.

### 쓰기 / 삭제 규칙

`put` 으로 신규 entry 를 만들면 owner 는 **호스트가 caller 로 도장찍는다**. caller 가 절대 명시할 수 없다.

기존 entry 를 갱신·삭제할 때는 owner check 가 발동한다:

| Caller | Entry owner | 결과 |
|--------|-------------|------|
| Plugin A | Plugin A | OK |
| Plugin A | Plugin B | `OwnedByOther { owner: "B" }` 에러 (JSON-RPC code `-32006`) |
| Plugin A | `_host` | `OwnedByOther` 에러 |
| `_host` (CLI) | anything | OK (root) |

`OwnedByOther` 는 권한 에러가 아니라 "이 entry 는 당신의 것이 아닙니다" 라는 의미. plugin 입장에서는 **갱신을 시도할 수 있지만 거부될 수 있는 명시적 실패 케이스** 다.

### 권한 토큰

| 토큰 | 보호 메서드 |
|------|-------------|
| `memory.read` | `memory.get`, `memory.list`, `memory.exists`, `memory.count`, `memory.scopes`, `memory.stats` |
| `memory.write` | `memory.put`, `memory.delete` |

`memory.write` 권한을 받았다고 해서 다른 plugin entry 를 쓸 수 있는 건 아니다. 권한은 "메서드 호출 가능 여부", owner check 는 "해당 entry 에 대한 권한". 두 단계가 모두 통과해야 쓰기가 성공한다.

## Secret memory: 플러그인별 사전 분할

Secret 은 권한 체크가 아니라 **아키텍처적 분할** 이다.

### 모델: "다른 plugin 의 secret" 은 개념 자체가 없다

플러그인 입장에서 secret memory 는 자기만의 독립된 우주다. 다음과 같이 생각하면 정확하다:

> **각 plugin 마다 자기 전용 `memory_secret` 테이블이 따로 존재한다.**
> Plugin A 가 `memory.secret.list` 를 호출하면 그게 secret memory 의 "전부" 다.
> Plugin B 의 secret 영역은 Plugin A 입장에서 **존재하지 않는 것이 아니라, 개념 자체가 없다**.

권한 거부도, "exists but hidden" 도 아니다. 다른 plugin secret 에 접근하는 **경로 자체가 IPC 표면에 노출되지 않는다** — `owner` 가 IPC 인자에 없으니까.

### 데이터 모델

```text
PK: (owner, scope, key)
columns: value, content_type, created_at, updated_at, expires_at, version
index: idx_memory_secret_owner ON memory_secret(owner)
```

`owner` 가 PK 의 일부이므로 owner 가 다른 두 plugin 이 **같은 `(scope, key)` 를 충돌 없이** 사용할 수 있다. 예: Plugin A 도 Plugin B 도 `(global, "api_token")` 을 가질 수 있다 — 둘은 서로 다른 row 이고, 서로의 존재를 모른다.

### 동작 규칙

호스트는 caller 에서 owner 를 도출한 뒤, 모든 secret 쿼리에 `WHERE owner = :caller_owner` 를 **자동으로 부착** 한다.

| 동작 | 결과 |
|------|------|
| Plugin A 가 `memory.secret.put(scope, key, value)` | 자기 row 에 upsert |
| Plugin A 가 `memory.secret.get(scope, key)` | 자기 row 만 검색 (없으면 `null`) |
| Plugin A 가 `memory.secret.list(scope)` | 자기 row 만 반환 |
| Plugin A 가 `memory.secret.scopes()` | 자기 row 의 scope 만 반환 |
| Plugin A 가 `memory.secret.stats()` | 자기 row 만 합산 |

응답에 `owner` 필드는 **포함되지 않는다**. plugin 은 그 차원이 존재한다는 사실을 알 필요가 없다.

### Local caller 의 secret

`_host` 도 secret 영역을 가질 수 있다 (owner = `_host`). CLI 로 `tasty memory secret put ...` 을 하면 `_host` 의 secret 으로 들어간다. 다만 일반적으로 `_host` 영역은 잘 안 쓰일 가능성이 높다 (사용자가 secret 으로 따로 보관할 데이터는 보통 plugin 이 만든다).

### 권한 토큰

| 토큰 | 보호 메서드 |
|------|-------------|
| `memory.secret` | `memory.secret.{put, get, delete, list, exists, count, scopes, stats}` |

R/W 를 토큰 하나로 묶는 이유: secret 은 항상 "자기 영역 only" 라서 read 와 write 를 분리할 이유가 없다. 권한이 있으면 자기 영역의 모든 동작이 가능하고, 권한이 없으면 secret 영역 자체에 접근 불가.

## CLI 표면

Regular:

```text
tasty memory put <scope> <key> <value> [--json | --binary]
tasty memory get <scope> <key>
tasty memory delete <scope> <key> [--cas N]
tasty memory list <scope> [--prefix P] [--limit N]
tasty memory exists <scope> <key>
tasty memory count <scope> [--prefix P]
tasty memory scopes
tasty memory stats [--scope S]
```

Secret:

```text
tasty memory secret put <scope> <key> <value> ...
tasty memory secret get <scope> <key>
tasty memory secret delete <scope> <key> ...
tasty memory secret list <scope> ...
tasty memory secret exists <scope> <key>
tasty memory secret count <scope> ...
tasty memory secret scopes
tasty memory secret stats [--scope S]
```

**`--owner` 같은 플래그는 존재하지 않는다.** CLI 는 `_host` 로 동작하며, regular 쪽에서 root 권한으로 모든 entry 를 조작할 수 있다. secret 쪽에서는 자기 (`_host`) 영역에만 접근한다.

조회/디버깅 용도로 특정 plugin 의 regular entry 만 보고 싶다면, 응답에 owner 가 포함되므로 grep/jq 로 사후 필터링한다. 별도 `--filter-owner` 플래그는 처음에는 제공하지 않는다 — 필요해지면 추가.

## 응답 스키마 차이

### Regular: `owner` 포함

```json
{
  "scope": "workspace:1",
  "key": "task.plan",
  "value": { ... },
  "content_type": "application/json",
  "created_at": 1700000000000,
  "updated_at": 1700000000123,
  "expires_at": null,
  "version": 3,
  "owner": "com.tasty.claude"
}
```

### Secret: `owner` 제외

```json
{
  "scope": "workspace:1",
  "key": "api_token",
  "value": "...",
  "content_type": "text/plain",
  "created_at": 1700000000000,
  "updated_at": 1700000000123,
  "expires_at": null,
  "version": 1
}
```

`owner` 필드의 존재 자체가 secret 모델의 추상화 누수가 된다. plugin 이 자기 owner 값을 알 필요가 전혀 없고, 알게 되면 "그럼 owner 를 명시하는 메서드는 없나?" 같은 잘못된 기대를 만든다.

## 에러 카탈로그

| `MemoryError` variant | JSON-RPC code | 의미 |
|---|---|---|
| `NotFound { scope, key }` | `-32004` | delete/update 대상 entry 가 없음 |
| `CasConflict { expected, actual }` | `-32005` | 낙관적 락 미스 |
| `OwnedByOther { owner }` | `-32006` | regular entry 의 owner 가 caller 와 다름 (root 아닌 경우) |
| `QuotaExceeded { used, limit, scope }` | `-32007` | secret (plugin 별) 또는 regular (global) 용량 초과. `scope` = `"secret"` \| `"regular"` |
| `SecretUnavailable` | `-32008` | keyring 미가용 + `allow_plaintext_secret = false` 상태에서 secret 메서드 호출 |
| `InvalidKey(_)` | `-32602` | invalid params |
| `InvalidScope(_)` | `-32602` | invalid params |
| `InvalidContentType(_)` | `-32602` | invalid params |
| `ValueTooLarge { actual, max }` | `-32007` | 단일 entry 1 MiB 초과 (quota 와 같은 코드, `reason` 으로 구분) |
| `Db(_)` | `-32603` | internal error |

## 마이그레이션 정책

`memory.db` 는 아직 릴리스에 포함된 적이 없다. SCHEMA_VERSION 은 1 에 머무르고, 0.x experimental 정책에 따라 single `SCHEMA_SQL` 을 적용한다. 따라서 owner 컬럼 / `memory_secret` 테이블 추가는 **누적 migration 없이** 스키마 정의를 갱신하면 끝난다.

1.0 freeze 시점에 누적 migration 체인으로 전환한다 (현재 `migrations.rs` 의 주석 참조).

## 보안·신뢰 모델

### 위협 모델

| 시나리오 | Regular | Secret |
|---|---|---|
| 다른 OS user 가 `~/.tasty/memory.db` 접근 | filesystem 권한으로 차단 | filesystem 권한으로 차단 |
| 같은 OS user 의 다른 프로세스가 DB 파일 직접 열기 | 평문 노출 | **AES-GCM 으로 보호** (키체인 접근 필요) |
| `~/.tasty/` 가 백업/클라우드 동기화 대상에 포함 | 평문 노출 | **차단** (디바이스 키체인 없는 백업본은 복호화 불가) |
| 디바이스 도난, 디스크 암호화 없음 | 평문 | **차단** |
| Plugin A 가 IPC 로 plugin B 의 secret 요청 | — | 차단 (개념 자체가 없음) |
| Plugin A 가 IPC 로 plugin B 의 regular entry 갱신 | `OwnedByOther` 로 차단 | — |

Secret 의 위협 모델은 **"디스크에 정착한 secret 데이터"** 보호다. "런타임에 메모리 위에서 plugin 이 자기 secret 을 읽고 있을 때 다른 프로세스가 훔쳐본다" 같은 시나리오는 OS 책임이지 Tasty 책임이 아니다.

### Secret 암호화

Secret 영역의 `value` blob 은 **application-level AES-256-GCM** 으로 암호화한다. SQLCipher 같은 page-level 암호화 대신 application-level 인 이유:

- Regular 와 Secret 이 같은 DB 파일에 산다. Regular 까지 같이 암호화할 이유가 없음.
- `secret` 테이블의 `scope` / `key` / `created_at` 등 metadata 는 평문이어도 위협 가치가 낮음 — 어차피 IPC schema 로 keyspace 가 알려져 있다.
- 핵심 가치는 `value` 자체. value blob 만 외과적으로 암호화.

**암호화 구조**:

```text
encrypted_blob = nonce(12B) || ciphertext || tag(16B)
```

- 알고리즘: AES-256-GCM (NIST 표준, AEAD)
- nonce: 매 write 마다 새로 생성 (random 12 bytes)
- AAD (associated data): `owner || scope || key` 의 SHA-256 — entry 정체성을 ciphertext 에 묶어 row 단위 swap 공격 방지
- master key: 32 bytes, OS 키체인에 보관

`content_type` 컬럼은 평문 유지 (Text/Json/Binary 분기에 필요). `version` `expires_at` 등 메타도 평문.

### 마스터 키 보관

`keyring` 크레이트로 OS 키체인에 master key 보관:

| OS | Storage |
|---|---|
| macOS | Keychain (`com.tasty.memory` / `master-key`) |
| Windows | Credential Manager |
| Linux | Secret Service (GNOME Keyring, KWallet, KeePassXC 등) |

첫 실행시 키가 없으면 생성하여 저장. 이후 실행마다 키체인에서 로드.

### Keyring 부재 시 정책

Linux 헤드리스·SSH·CI 환경에서는 Secret Service 가 없을 수 있다. 이 경우 **명시적 에러로 실패**한다:

- 시작 시 keyring 가용 여부 probe → 실패하면 `MemoryInitError::KeyringUnavailable`.
- 호스트는 사용자에게 안내 메시지 표시 후 **secret 기능을 disable 상태로 시작**한다 (regular 영역은 정상 동작).
- Secret IPC 메서드 호출 시 `SecretUnavailable` 에러 반환.
- 사용자가 의도적으로 평문 폴백을 원하면 `config.toml` 의 `[memory] allow_plaintext_secret = true` 명시 설정 필요. 이 경우 `~/.tasty/.secret-key` 에 키를 평문 저장하고 시작 시 warning 로그 + UI 알림.

평문 폴백을 default 로 두지 않는 이유: `secret` 이라는 이름은 약속이다. 약속을 깨려면 사용자가 명시적으로 동의해야 한다.

### 신뢰 한계

- **Plugin process 가 호스트를 우회해 직접 `memory.db` 를 여는 경우**는 막지 않는다. 다만 secret value 는 암호화돼 있어 키체인 접근 권한이 없으면 못 읽는다. 키체인은 같은 OS user 권한이면 일반적으로 접근 가능하므로, 결국 **악의적 native plugin** 시나리오는 부분적으로만 차단된다. 이런 신뢰 한계는 plugin 권한 시스템 전체에 공통.
- `memory.write` 권한이 있어도 `OwnedByOther` 로 막힌다는 점은, plugin 간 데이터 격리를 권한 grant 의 미세 조정 없이도 강제한다. "이 plugin 에게 memory write 권한을 주면 다른 plugin 데이터를 망가뜨릴 수 있나?" → 못 한다.

## 용량 제한

용량 제한은 세 층으로 작동한다:

| 제한 | 대상 | 기본값 | 초과시 |
|---|---|---|---|
| 단일 entry max | 1 entry 의 `value` byte | 1 MiB | `ValueTooLarge { actual, max }` |
| Plugin secret quota | plugin 별 `memory_secret` 의 `SUM(LENGTH(value))` | 10 MiB | `QuotaExceeded { used, limit, scope: "secret" }` |
| Regular global quota | 전체 `memory` 의 `SUM(LENGTH(value))` | 1 GiB | `QuotaExceeded { used, limit, scope: "regular" }` |

기본값은 fallback 으로만 하드코드되며, 모든 값은 config 로 재정의 가능하다.

### 정책

- **단일 entry max** 는 메모리 사용량·직렬화 비용 보호용 cap. config 로 조정 가능 (default 1 MiB).
- **Secret quota 는 plugin 별 개별 제한.** Plugin A 가 자기 quota 를 다 썼다고 Plugin B 가 영향받지 않는다.
- **Regular quota 는 global.** Regular 는 plugin 간 공유 네임스페이스라 owner 별로 quota 를 쪼개면 "다른 plugin 이 다 채워서 내가 못 쓰는" surprise 가 생긴다. 단일 cap 으로 전체 비대만 막는다.
- **Eviction 안 함.** 초과시 명시적으로 `QuotaExceeded` 에러. plugin 데이터가 말 없이 사라지는 surprise 방지.
- **Local caller (`_host`) 도 quota 를 받는다.** root 권한은 owner check 에만 적용되고, 디스크 비대 방지는 동일하게 enforce.

### 설정

`~/.tasty/config.toml`:

```toml
[memory]
# 단일 entry 의 value 최대 byte (default: 1 MiB)
entry_max_mb = 1

# 각 plugin 의 secret 영역 최대 byte (default: 10 MiB)
secret_quota_mb_per_plugin = 10

# Regular 영역 총합 최대 byte (default: 1 GiB)
regular_quota_mb_total = 1024

# Linux keyring 부재시 평문 폴백 허용 (default: false)
allow_plaintext_secret = false
```

용량 값은 `MiB` 단위 정수. 0 또는 음수는 invalid 로 reject. 누락된 항목은 위 default 로 폴백.

### Enforcement

`put` / `put_secret` 직전에 SUM(LENGTH(value)) 쿼리:

```sql
-- secret quota check
SELECT COALESCE(SUM(LENGTH(value)), 0) FROM memory_secret WHERE owner = :caller_owner;

-- regular quota check
SELECT COALESCE(SUM(LENGTH(value)), 0) FROM memory;
```

기존 entry 의 UPDATE 인 경우 기존 byte 를 빼고 새 byte 를 더해 계산. `idx_memory_secret_owner` 인덱스로 secret quota 쿼리는 owner row 만 스캔.

## 관련 문서

- 코드: `crates/tasty-memory/`
- 권한 모델 전체: [dev-guide/plugin-permissions.md](../dev-guide/plugin-permissions.md)
- IPC/CLI 레퍼런스: [agent-guide/api-reference.md](../agent-guide/api-reference.md) (`memory.*` 섹션)
- 저장 위치: [design/storage-system.md](storage-system.md)
