# Plugin 권한 모델 — 개발자 가이드

Plugin이 호스트 IPC를 호출할 때 적용되는 권한 게이트의 동작 원리와 새 IPC 메서드를
추가할 때 따라야 할 절차.

## 구성 요소

| 위치 | 역할 |
|------|------|
| `src/plugin/manifest.rs::Permission` | 권한 enum + 토큰 매핑. 새 권한 카테고리 추가 시 여기 |
| `src/ipc/method_meta.rs` | IPC 메서드 → 필요 권한 / plugin 호출 가능 여부 매핑 (단일 진실 원천) |
| `src/ipc/caller.rs::CallerContext` | 호출자 종류 (Local / Internal / Plugin{plugin_id, permissions} / Agent{agent_id, parent, permissions, token}) |
| `src/ipc/handler/mod.rs::handle_with_caller` | 라우터 진입에서 ensure_allowed 호출 + capability_elevation 자동 발행 + audit 기록 |
| `src/plugin/manager.rs::PluginManager::plugin_permissions` | plugin id → Arc<HashSet<Permission>> 캐시 |
| `src/plugin/registry_state.rs::PluginsConfig::grants` | `~/.tasty/plugins.toml`의 grant 영속화 |
| `src/ipc/session.rs::SessionStore` | agent session token + base permissions + temp_grants (`tasty.session.<token>` Global scope) |
| `src/ipc/audit.rs::AuditStore` | IPC 호출 audit log (`tasty.audit.{ts:013}.{seq:04}` Global scope, 기본 30일 retention) |

## 권한 토큰 형식

매니페스트의 `permissions = [...]` 에 들어가는 문자열 (이하 **토큰**) 의 전체 형식:

```
<name>[:<scope>]
```

두 기호의 의미가 다르다:

| 기호 | 의미 | 설명 |
|---|---|---|
| `.` | **이름의 일부** | 토큰 식별자 안의 namespace dot. `surface.read` 의 `.` 는 "surface 카테고리의 read" 라고 분류해 부르는 단일 이름의 일부일 뿐, 호스트는 토큰을 dot 로 쪼개서 처리하지 않는다. |
| `:` | **scope 구분자** | 권한 이름과 그 권한이 적용되는 "대상" 을 가른다. 권한 자체로는 의미가 너무 광범위해서, 적용 대상을 한정해야 의미가 생기는 권한에만 등장. |

### Scope 없는 토큰 (단순 enum variant)

대부분의 권한은 scope 없는 고정 문자열로 정의돼 있다. `Permission::from_token` (`src/plugin/manifest.rs`) 의 match arm 에 정확 일치로만 등록된다 — `surface.read` 라고 적으면 매칭되지만 `surface.Read` 나 `surface_read` 는 거부.

현재 등록된 단순 토큰 전부:

```
surface.read       surface.write
clipboard.read     clipboard.write
fs.read            fs.write
terminal.spawn     terminal.write      terminal.read
process.spawn
memory.read        memory.write        memory.secret
notification
network
ui.popup           ui.tool_item
approval
telemetry
agent
file_handler.define
```

새 단순 권한을 추가하려면 `Permission` enum 에 variant 를 추가하고 `from_token` / `as_token` match 에 등록한다 (아래 "새 권한 카테고리 추가" 절차 참조).

### Scope 있는 토큰 (파라미터화 권한)

현재 scope 를 받는 토큰:

| 토큰 | scope 의미 | 예 |
|---|---|---|
| `ipc.invoke:<prefix>` | 호출 대상 IPC namespace prefix | `ipc.invoke:codex` → codex.* 메서드 호출 가능 |
| `ext:<plugin_id>` | 확장 대상 plugin 의 reverse-DNS id | `ext:com.tasty.clipboard` → 해당 plugin 의 IPC/event 흐름에 hook |
| `file_handler.extend:<detector_id>` | 기존 detector 에 rule 추가 | `file_handler.extend:markdown` |
| `file_handler.handle:<detector_id>` | 기존 detector 에 handler attach | `file_handler.handle:pdf` |

scope 부분의 검증 규칙:

- `ipc.invoke:` 뒤: `is_valid_ipc_prefix()` 통과 (소문자로 시작 + 소문자/숫자/`_`, 1~32자) **그리고** `is_reserved_ipc_prefix()` 거부 (`plugin`, `system`, `surface`, `tab`, `pane`, `workspace`, `split`, `tree`, `hook`, `global_hook`, `message`, `tool`, `notification`, `window`, `debug`, `ui`, `ime`, `ipc`, `memory`, `output`, `approval`)
- `ext:` 뒤: `is_valid_plugin_id()` 통과 (reverse-DNS 형식, 소문자+숫자+`-`+`.` 의 1~3 segment)
- `file_handler.extend:` / `file_handler.handle:` 뒤: `is_valid_detector_id()` 통과, `$unknown` 은 거부 (실재하지 않는 sentinel)

조건을 어긋난 토큰이 매니페스트에 들어 있으면 plugin 로드 단계에서 거부된다 (`Permission::from_token` 가 `None` 반환 → `validate_permissions` 에서 reject).

## Scope 의 출처

**`ipc.invoke:<X>` 의 `X` 는 호스트가 미리 정의한 enum 이 아니다.** 각 plugin 의 매니페스트가 자기 namespace 를 `[[contributes.ipc_namespace]]` 로 선언함으로써 비로소 `X` 라는 scope 이름이 시스템에 존재하기 시작한다. 즉 scope 는 **plugin 생태계가 동적으로 만들어내는 이름공간** 이다.

### 한 plugin 의 lifecycle

```
1. tasty-plugin-image 매니페스트:
     [[contributes.ipc_namespace]]
     prefix = "image"
                ↓
2. 호스트가 "image" namespace 를 image plugin 에 귀속
   (이 시점에 "image" 라는 이름이 IPC scope 로 의미를 가진다)
                ↓
3. 다른 plugin (예: gallery) 매니페스트:
     permissions = ["ipc.invoke:image"]
                ↓
4. 사용자 grant → gallery plugin 이
   host.call("image.open", ...) 호출 가능
```

### Host 가 검증하는 것 / 검증하지 않는 것

매니페스트 parse 시 `permissions = ["ipc.invoke:image"]` 를 만났을 때:

| 검증한다 | 검증하지 않는다 |
|---|---|
| `image` 가 형식적으로 valid 한가 (소문자+숫자+`_`, ≤32자) | `image` 라는 namespace 를 점유한 plugin 이 실제로 설치돼 있는가 |
| `image` 가 호스트 예약어가 아닌가 | 그 plugin 이 enable 상태인가 |
| 토큰 prefix (`ipc.invoke:`) 가 알려진 권한 이름인가 | 그 plugin 이 지금 running 인가 |

owner 존재를 검증하지 **않는** 의도적 이유:

- **install 순서 무관성.** plugin A 가 plugin B 의 namespace 를 호출하는 경우, B 가 A 보다 늦게 설치돼도 A 의 매니페스트가 거부되면 안 된다. 사용자가 plugin 을 순서 가리지 않고 깔 수 있어야 한다.
- **disable/enable cycle 견고성.** B 를 잠깐 disable 했을 때 A 의 매니페스트가 재검증으로 떨어지면 안 된다.
- **dangling reference 는 runtime 에 명확히 실패한다.** owner 가 없는 namespace 를 호출하면 `-32601 method not found` 가 반환된다. silent 실패 / 흐릿한 에러가 아니다.

### Scope owner 의 unique 보장

같은 prefix 를 두 plugin 이 동시에 contribute 할 수 없다. 두 번째 plugin install 시점에 호스트가 충돌을 감지해 거부한다 (`agent-guide/plugins.md` 의 "중복 점유 거부" 절 참조). 따라서 임의 시점에 `image` 라는 scope 는:

- 정확히 한 plugin 에 귀속 (정상 상태), 또는
- 아무에게도 귀속되지 않음 (모든 contribute plugin 미설치 / disable 상태)

둘 중 하나다. 한 scope 가 둘 이상의 plugin 에 동시 매핑되는 상태는 만들어질 수 없다.

### Self-namespace permission 의 무용성

자기 namespace 의 `ipc.invoke:<self>` 토큰을 자기 매니페스트에 두는 것은 무용하다. 호스트가 self-loop 를 차단하기 때문이다 (`-32001` 에러). plugin 이 자기 메서드를 부르려면 IPC 우회 없이 코드에서 직접 `handle_ipc_method` 안의 함수를 호출하면 된다. 따라서:

```toml
# 안티패턴 — image plugin 자기 매니페스트
permissions = [..., "ipc.invoke:image"]   # ❌ 무용
```

호스트는 이 토큰을 거부하지는 않지만 (형식상 valid), 권한 grant prompt 의 정보 비대만 만든다. plugin 작성자는 self-namespace 토큰을 매니페스트에 두지 않는다.

## 새 권한 토큰 형식을 추가할 때

scope 가 필요한 새 권한을 추가할 때 (드물지만 발생할 수 있음):

1. `Permission` enum 에 `<Name>(String)` variant 추가.
2. `from_token` 의 fallback (`other => ...`) 안에서 `strip_prefix("<prefix>:")` 매칭 추가, scope 부분 검증 함수 호출.
3. `as_token` 에 `format!("<prefix>:{scope}")` 추가.
4. scope 값의 유효성 검증 함수 (`is_valid_<x>`) 작성 — 형식만 검증. owner 존재 여부는 검증하지 않는다 (위 "install 순서 무관성" 이유).
5. 권한이 강제하는 동작 (runtime 게이트) 을 `manager.rs` 등에 추가.
6. `agent-guide/plugins.md` 의 권한 표 갱신.

이 패턴은 `ipc.invoke` / `ext` 두 사례 모두 동일하게 구현돼 있다 — 새로 추가할 때 reference 로 삼는다.

## 새 IPC 메서드 추가 절차

핸들러를 추가했다면 **반드시** `src/ipc/method_meta.rs::method_meta`에도 매핑을 등록해야 한다.

```rust
// src/ipc/method_meta.rs
"surface.my_new_method" => plugin(&[Permission::SurfaceWrite]),
```

매핑이 누락된 메서드는 `method_meta()`가 `None`을 반환하며, plugin이 호출 시 자동으로 `UnknownMethod` 거부된다. Local 호출은 매핑 없이도 통과(라우터의 `method_not_found`로 fallthrough)하지만, plugin은 절대 호출할 수 없으므로 **plugin 호출이 필요한 메서드라면 반드시 등록**해야 한다.

debug 메서드 / 호스트 자체 메서드(`plugin.*`, `window.*`)는 `local_only()`로 등록한다.

## 매니페스트 contributes 권한 게이트

IPC 메서드 외에도 `contributes` 일부 항목은 권한 매핑을 강제한다. 검증은
plugin 로드 단계에서 `manifest::validate_*_permissions` 함수가 수행하며,
누락 시 plugin 시작이 거부된다.

| contributes 항목 | 요구 권한 | 검증 함수 |
|----------------|----------|----------|
| `events_emitted[].key` | 매니페스트 `event_publish` 패턴이 해당 키를 커버해야 함 | `validate_events_emitted` |

surface lifecycle 알림 같은 broadcast 이벤트는 Event Bus의 `event_subscribe` 패턴으로 받는다 (요구 권한 별도 없음 — 패턴 자체가 권한 게이트).

새 contributes 항목을 추가할 때 권한 게이트가 필요하다면 동일 패턴으로 검증 함수를 만들고 `PluginManifest::validate_permissions`에서 호출한다.

## 새 권한 카테고리 추가

1. `Permission` enum에 variant 추가
2. `Permission::from_token` / `as_token`에 토큰 매핑 추가
3. `method_meta` 테이블에서 해당 권한을 사용하는 메서드 갱신
4. `docs/agent-guide/plugins.md`의 권한 표 갱신

## 흐름

```
plugin → PluginEvent::IpcCall { call_id, method, params }
       → manager.pending_plugin_calls 에 push
       → App::process_plugin_ipc_calls()
         → CallerContext::Plugin { plugin_id, permissions: Arc::clone(&cache) }
         → ipc::handler::handle_with_caller(state, request, caller)
           → caller.ensure_allowed(method)?
             → method_meta(method)?
             → plugin_callable 검사
             → required permissions ⊆ caller.permissions 검사
           → route_engine_handler / route_gui_handler / route_debug_handler
       → manager.send_ipc_result(plugin_id, call_id, result, error)
       → plugin이 ipc.result 요청 수신
```

## 권한 변경 즉시 반영

`PluginManager::plugin_permissions`는 `HashMap<String, Arc<HashSet<Permission>>>`이다.
사용자가 grant/revoke를 하면:

1. `PluginsConfig::grant`/`revoke` 호출 → `plugins.toml` 저장
2. `refresh_plugin_permissions(mgr, id)` 호출 → 매니페스트 권한 ∩ granted를 새 `HashSet`으로 만들어 `Arc::new`로 교체

`CallerContext::Plugin`이 호출 시점에 `Arc::clone`을 보유하므로, 호출 도중 다른 스레드가 해시셋을 갱신해도 **현재 호출은 일관된 권한 set을 사용**한다 (snapshot semantics).

## 테스트

- `src/plugin/manifest.rs::tests::rejects_unknown_permission` — 매니페스트 검증
- `src/plugin/manifest.rs::tests::parsed_permissions_returns_enum_set` — token 파싱
- `src/plugin/registry_state.rs::tests::grant_revoke_round_trip` — grant 영속화
- `src/ipc/method_meta.rs::tests::*` — 매핑 sanity check
- `src/ipc/caller.rs::tests::*` — Local 통과, Plugin 거부, debug/local-only 분리

새 권한 / 메서드를 추가했다면 권장:

- `caller.rs`의 `plugin_with(&[Permission::X])`으로 새 메서드가 정확한 권한을 요구하는지
- 권한 부족 시 `MissingPermission` 에러가 적절한 토큰을 가리키는지

## Agent caller — session token + temp grants

`claude.spawn` 같은 호스트 launched 자식 프로세스는 launch 시 `session.issue` 로 64-char hex token 을 받아 `TASTY_SESSION_TOKEN` 환경변수로 전달받는다. 자식이 IPC 호출 시 envelope 의 `session_token` 필드로 첨부 → 호스트 dispatcher 가 `SessionStore::resolve` 로 `CallerContext::Agent { agent_id, parent, permissions, token }` 을 만든다.

```
[host: claude.spawn]
  → session.issue { agent_id: "claude:child-1", permissions: [...] }
  → SessionStore::issue → token 발급, AgentSession {base_permissions, temp_grants:[]} 영속
  → 자식 process spawn (TASTY_SESSION_TOKEN=<hex> 주입)

[child: IPC 호출]
  → envelope.session_token = "<hex>"
  → 호스트: SessionStore::resolve(token) → AgentSession
  → CallerContext::Agent { agent_id, parent, permissions: base ∪ effective_temp_grants(now), token }
  → ensure_allowed 평가
```

invalid / expired / revoked 토큰은 `-32001 permission_denied` 로 즉시 거부 — `CallerContext::Local` 로 fallback 하지 않는다 (환경변수 위조 방어).

### Base vs temp_grants

`AgentSession` 은 두 권한 슬롯을 분리해 둔다:

- **base_permissions** — `session.issue` 시점에 정해진 권한. caller (Plugin / Agent) 자기 권한의 부분집합만 가능 (escalation 방지). Local 은 무제한.
- **temp_grants: Vec<TempGrant{permission, expires_at_ms?}>** — runtime 에 `plugin.grant_agent_permission` 으로 추가. 만료 시점이 지나면 lazy evict (`resolve`/`list` 시).

`effective_permission_set(now_ms) = base ∪ {non-expired temp_grants}`. 매 IPC 호출이 `resolve` 를 거치므로 별도 snapshot refresh 불필요.

같은 token+permission 으로 grant 가 반복 호출되면 만료 시점만 갱신 — 가장 늦은 시점 또는 `None` (무기한) 우선. base 에 이미 있는 토큰은 noop (temp 슬롯 오염 방지).

### Capability elevation 자동 발행

`handle_with_caller` 의 `ensure_allowed` 거부 분기 안에서:

```rust
if caller is Agent && error is MissingPermission(perm) {
    publish_capability_elevation(state, agent_id, method, perm, reason)
        .map(|approval_id| error.data = {kind, approval_id, permission, method})
}
```

`publish_capability_elevation` 은:

1. 같은 `(agent_id, permission)` 의 Pending elevation 이 있으면 그 `approval_id` 재사용 (popup 폭주 방지).
2. 없으면 `approval.request { kind: "capability_elevation", choices: [approve, approve_permanently, deny], metadata: { permission, agent_id, method, grant_ttl_secs: 3600 } }` 호출.

`approval.respond` 에서:

- `approve` → `metadata.grant_ttl_secs` 만큼 `SessionStore::grant_permission` (default 3600s).
- `approve_permanently` → 무기한 grant (`ttl_ms=None`).
- `deny` → grant 없음, 다음 호출도 거부.

순수 결정 함수 `elevation_grant_decision(record, choice) -> Option<(agent_id, permission, ttl_ms)>` 와 I/O wrapper `apply_elevation_grant_if_any(record, choice)` 로 분리돼 unit test 가 grant 결정 로직만 검증 가능.

## Audit log

`handle_with_caller` 가 3 경로 모두에서 `audit::record` 를 호출한다:

```rust
// ensure_allowed deny
crate::ipc::audit::record(caller, canonical, AuditDecision::Deny, Some(&format!("{e}")), workspace_id, seq);

// cap_blocked deny (Phase 4.3c)
crate::ipc::audit::record(caller, canonical, AuditDecision::Deny, Some(&format!("cap_blocked: {reason}")), workspace_id, seq);

// allow
crate::ipc::audit::record(caller, canonical, AuditDecision::Allow, None, workspace_id, seq);
```

`main.rs::process_ipc` 의 app-level plugin.* 라우터에서도 동일 hook (audit_query 자체도 기록 — query 호출이 audit 에 노이즈로 들어가지만 query 시 filter 가능). `seq` 는 `state.engine.telemetry_seq` 를 그대로 사용 — 같은 ms 안의 다중 호출도 단조 정렬.

저장은 `AuditStore::append` → `tasty_memory::with_store` 로 `tasty.audit.{ts:013}.{seq:04}` Global scope. 운영자가 `plugin.audit_query / summary / follow / clear` 로 조회. 기본 30일 retention — `audit_query` 호출 시 lazy evict (별도 정리 스레드 없음).

## 한계

권한 게이트는 **호스트 IPC 호출만 막는다**. Plugin이 자기 프로세스에서 직접 `std::fs::write`로 임의 경로에 쓰면 호스트는 알 수 없다. 진정한 격리는 OS-level 샌드박스(seccomp / sandbox-exec / WASM 등)가 필요하며 현재 범위 외다.

따라서 매니페스트의 `permissions[]`는 **"호스트 API를 호출할 권한"** 이지 "OS 자원에 대한 시스템 권한"이 아니다. UI/문서에서 사용자에게 grant를 요청할 때 이 표현을 유지해야 false security를 만들지 않는다.

1.0까지의 정책 선택과 재검토 trigger는 [plugin-ecosystem.md §3](plugin-ecosystem.md) 참조.
