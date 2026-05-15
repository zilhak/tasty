# Plugin 권한 모델 — 개발자 가이드

Plugin이 호스트 IPC를 호출할 때 적용되는 권한 게이트의 동작 원리와 새 IPC 메서드를
추가할 때 따라야 할 절차.

## 구성 요소

| 위치 | 역할 |
|------|------|
| `src/plugin/manifest.rs::Permission` | 권한 enum + 토큰 매핑. 새 권한 카테고리 추가 시 여기 |
| `src/ipc/method_meta.rs` | IPC 메서드 → 필요 권한 / plugin 호출 가능 여부 매핑 (단일 진실 원천) |
| `src/ipc/caller.rs::CallerContext` | 호출자 종류 (Local / Plugin{plugin_id, permissions}) |
| `src/ipc/handler/mod.rs::handle_with_caller` | 라우터 진입에서 ensure_allowed 호출 |
| `src/plugin/manager.rs::PluginManager::plugin_permissions` | plugin id → Arc<HashSet<Permission>> 캐시 |
| `src/plugin/registry_state.rs::PluginsConfig::grants` | `~/.tasty/plugins.toml`의 grant 영속화 |

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

## 한계

권한 게이트는 **호스트 IPC 호출만 막는다**. Plugin이 자기 프로세스에서 직접 `std::fs::write`로 임의 경로에 쓰면 호스트는 알 수 없다. 진정한 격리는 OS-level 샌드박스(seccomp / sandbox-exec / WASM 등)가 필요하며 현재 범위 외다.

따라서 매니페스트의 `permissions[]`는 **"호스트 API를 호출할 권한"** 이지 "OS 자원에 대한 시스템 권한"이 아니다. UI/문서에서 사용자에게 grant를 요청할 때 이 표현을 유지해야 false security를 만들지 않는다.

1.0까지의 정책 선택과 재검토 trigger는 [plugin-ecosystem.md §3](plugin-ecosystem.md) 참조.
