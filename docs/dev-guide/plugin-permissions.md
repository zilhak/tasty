# 플러그인 권한 모델

플러그인이 호스트 IPC 를 호출할 때 적용되는 권한 게이트의 동작 원리 + 새 IPC 메서드/토큰 추가 절차. 토큰 목록·개념은 [concepts/plugins](../concepts/plugins.md#권한-permissions), 제작 흐름은 [plugin-development](plugin-development.md).

## 구성 요소

| 위치 | 역할 |
|------|------|
| `crates/tasty-plugin-manifest/src/types.rs::Permission` | 권한 enum + 토큰 매핑(`from_token`/`as_token`). 새 토큰은 여기 |
| `crates/tasty-ipc/src/method_meta.rs::method_meta` | IPC 메서드 → 필요 권한 / plugin 호출 가능 여부 (단일 진실원) |
| `crates/tasty-ipc/src/caller.rs::CallerContext` | 호출자 종류 (Local / Internal / Plugin / Agent) + `ensure_allowed` |
| `src/adapters/ipc/handler.rs::handle_with_caller` | 라우터 진입에서 `ensure_allowed` + capability elevation 자동 발행 + audit |
| `crates/tasty-host-plugin/src/manager.rs::plugin_permissions` | plugin id → `Arc<HashSet<Permission>>` 캐시 |
| `crates/tasty-host-plugin/src/registry_state.rs` | `plugins.toml` 의 grant 영속화 |

## 토큰 형식 — `<name>[:<scope>]`

- **`.`** 는 *이름의 일부*다 (`surface.read` 의 `.` 는 분류용 — 호스트는 쪼개지 않음).
- **`:`** 는 *scope 구분자*다 — 권한이 적용되는 대상을 한정해야 의미가 생기는 권한에만 등장.

전체 토큰 목록(scope 없는 24 + scoped 5)은 [concepts/plugins](../concepts/plugins.md#권한-permissions). scoped 검증 규칙:

- `ipc.invoke:<prefix>` — `is_valid_ipc_prefix`(소문자 시작+소문자/숫자/`_`, ≤32) **그리고** 호스트 예약어 거부.
- `ext:<plugin_id>` — `is_valid_plugin_id`(reverse-DNS).
- `file_handler.extend:<id>` / `file_handler.handle:<id>` — `is_valid_detector_id`, `$unknown` 거부.
- `hook_handler.handle:<id>` — `is_valid_hook_handler_id`(소문자+숫자+`-`, ≤32). `$`-prefix reserved 개념 없음. `hook_handler.define` 은 scope 없는 base 토큰.

형식 위반 토큰은 `from_token` 이 `None` → 매니페스트 로드 단계에서 거부.

## Scope 의 출처 — 동적 이름공간

`ipc.invoke:<X>` 의 `X` 는 호스트 enum 이 아니다. 각 플러그인이 `[[contributes.ipc_namespace]]` 로 prefix 를 선언함으로써 그 scope 이름이 시스템에 존재하기 시작한다. 호스트는 매니페스트 parse 시 **형식만 검증**하고 **owner 존재는 검증하지 않는다**:

| 검증한다 | 검증 안 한다 |
|----------|--------------|
| 형식 valid / 예약어 아님 | 그 namespace 를 점유한 플러그인이 설치/활성/running 인가 |

owner 미검증의 이유 — **install 순서 무관성**(B 가 A 보다 늦게 깔려도 A 매니페스트가 거부되면 안 됨), **disable/enable 견고성**, dangling 호출은 runtime 에 `-32601 method not found` 로 **명확히** 실패. 같은 prefix 는 두 플러그인이 동시에 점유 불가(두 번째 install 거부) — 임의 시점에 scope 는 정확히 한 플러그인에 귀속 또는 무소속.

**자기 namespace `ipc.invoke:<self>` 는 무용**(self-loop 를 `-32001` 로 차단) — 매니페스트에 두지 않는다.

## 새 IPC 메서드 추가 절차

핸들러를 추가하면 **반드시** `method_meta` 에 매핑 등록:

```rust
"surface.my_new_method" => plugin(&[Permission::SurfaceWrite]),
```

누락 메서드는 `method_meta` 가 `None` → plugin 호출 시 자동 `UnknownMethod` 거부(Local 은 fallthrough 통과). debug/호스트 자체 메서드(`plugin.*`/`window.*`)는 `local_only()`.

**권한은 "무엇을 실제로 건드리는가"로 정한다** — Surface 트리를 건드리면 `Surface*` 가 섞이고, 순수 PTY IO 만 하면 `Terminal*` 만 쓴다. headless PTY primitive(`pty.*`, [ADR-0050](../adr/0050-headless-pty-primitive.md))가 이 규칙의 예시다 — 새 `Pty*` 토큰 없이 기존 `Terminal*` 3종만 재사용한다:

| 메서드 | 권한 | Surface 를 건드리는가 |
|--------|------|----------------------|
| `pty.spawn` | `TerminalSpawn` | 아니오 (Surface 없이 PTY 만) |
| `pty.write` / `pty.kill` | `TerminalWrite` | 아니오 |
| `pty.read` / `pty.wait` / `pty.list` | `TerminalRead` | 아니오 |
| `pty.attach_surface` | `SurfaceWrite, TerminalSpawn` | **예** (실제 Tab 생성 — `terminal.spawn` 과 동일 이유로 `SurfaceWrite` 추가) |

## 새 권한 토큰 추가

1. `Permission` enum 에 variant 추가(scoped 면 `<Name>(String)`).
2. `from_token`/`as_token` 매핑(scoped 면 `strip_prefix` + scope 검증 함수).
3. `is_valid_<x>` 검증 함수 — **형식만**, owner 존재는 검증 안 함.
4. runtime 게이트(`method_meta` 또는 manager) 배선.
5. [concepts/plugins](../concepts/plugins.md#권한-permissions) 토큰 목록 갱신.

`ipc.invoke`/`ext` 두 사례가 reference.

## contributes 권한 게이트

IPC 외에 일부 contribute 는 권한을 강제(매니페스트 로드 단계 거부):

| contributes | 요구 권한 |
|-------------|-----------|
| `[[contributes.tool]]` | `ui.tool_item` |
| `[[contributes.popup]]` | `ui.popup` |
| `[[contributes.settings_pages]]` | `ui.settings_page` (카테고리 무관) |
| `[[contributes.window]]` | `window.spawn` |
| `[extends]` | `ext:<target>` |
| `[[contributes.detector]]` (신규) | `file_handler.define` |
| `[[contributes.hook_handler]]` | `hook_handler.define` |

`event_subscribe` 는 별도 권한 없음 — 패턴 자체가 게이트.

## Builtin 자동 grant

번들 플러그인 매니페스트 권한은 `install_builtins_if_needed` 가 자동 grant — 최초는 전체, 기존 사용자에 새 버전이 토큰을 추가하면 `apply_builtin_permission_diff` 가 **신규 토큰만 증분** grant(기존 deny 보존).

## 권한 변경 즉시 반영

grant/revoke → `plugins.toml` 저장 → `refresh_plugin_permissions` 가 (매니페스트 ∩ granted)를 새 `Arc<HashSet>` 로 교체. `CallerContext::Plugin` 이 호출 시점에 `Arc::clone` 을 쥐므로 호출 도중 갱신돼도 일관(snapshot semantics).

## Agent caller — session token + temp grants

`claude.spawn` 같은 호스트-launched 자식은 `session.issue` 로 64-char hex 토큰을 받아 `TASTY_SESSION_TOKEN` 으로 전달, IPC envelope 의 `session_token` 으로 첨부 → `CallerContext::Agent`. invalid/expired/revoked 는 `-32001` 즉시 거부(Local fallback 안 함 — 환경변수 위조 방어).

- **base_permissions**: `session.issue` 시점 고정. caller 권한의 부분집합만(escalation 방지).
- **temp_grants**: runtime `plugin.grant_agent_permission` 추가, 만료 lazy evict. `effective = base ∪ non-expired temp`.

**Capability elevation 자동 발행**: Agent 가 `MissingPermission` 으로 거부되면 `approval.request{kind:capability_elevation}` 발행(같은 (agent,permission) Pending 은 approval_id 재사용). `approve`(TTL grant) / `approve_permanently`(무기한) / `deny`.

## Audit log

`handle_with_caller` 가 allow/deny 양쪽에서 `audit::record` → `tasty.audit.{ts}.{seq}` Global scope(기본 30일, lazy evict). `plugin.audit_query/summary/follow/clear` 로 조회.

## 한계

권한 게이트는 **호스트 IPC 호출만** 막는다. 플러그인이 자기 프로세스에서 `std::fs::write` 로 임의 경로에 쓰면 호스트는 모른다 — 진짜 격리는 OS 샌드박스(seccomp/sandbox-exec/WASM)가 필요하고 현재 범위 밖. 즉 매니페스트 `permissions[]` 는 **"호스트 API 호출 권한"** 이지 "OS 자원 권한"이 아니다 — UI/문서에서 grant 요청 시 이 표현을 유지해 false security 를 만들지 않는다.
</content>
