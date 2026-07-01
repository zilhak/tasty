# Lua Hooks — 호스트 측 매핑

호스트가 Lua hook 을 발화하는 코드 경로와 wire payload 스키마. 사용자 가이드는 [features/lua-hooks](../features/lua-hooks/index.md), 설계 배경(observe-only)은 [design/policies/lua-hooks](../design/policies/lua-hooks.md).

## 구성

```
crates/tasty-lua/
  src/engine.rs    # LuaEngine — 워커 스레드 핸들 + job/커맨드 큐 + fire()
  src/host_api.rs  # tasty.log / warn / run_cli(커맨드 큐) / tree(스냅샷)
  src/sandbox.rs   # 메모리 cap + 위험 글로벌 제거
  src/bridge.rs    # LuaSnapshot / HostCommand (GUI 비의존 마샬링 타입)
  meta/tasty.lua   # EmmyLua stub (LuaLS 용)
```

`App` 가 `lua_engine: Option<LuaEngine>` 를 보유(`src/app.rs`). 부팅 시 `LuaEngine::new()` 로 VM 을 전용 워커 스레드에 기동한다 — 부팅 자동로드(init.lua)는 폐기됐다(ADR-0031). 메인은 `about_to_wait` 안전지점에서 읽기 스냅샷 발행(`publish_lua_snapshot`)과 워커 커맨드 drain(`dispatch_pending_lua_commands`)을 수행한다.

이벤트 발화는 `fire_lua` 헬퍼 한 곳을 거친다(`src/app/dispatch/host_events.rs`):

```rust
fn fire_lua<T: Serialize>(lua: Option<&LuaEngine>, event: &str, payload: &T)
```

`payload` 는 `serde_json::Value` 직렬화 후 Lua table 로 변환 — wire 필드 ↔ Lua table 필드 1:1(snake_case).

## 이벤트 ↔ 발화 site

대부분 `dispatch_pending_host_events`(`src/app/dispatch/host_events.rs`)가 `PendingHostEvent` 를 소비하며 발화한다:

| 이벤트 | Payload |
|--------|---------|
| `tasty.startup.post` | Null |
| `window.create.post` / `window.delete.post` | `WindowCreated` / `WindowClosed` |
| `workspace.{create,delete}.post` | `WorkspaceCreated` / `WorkspaceClosed` |
| `workspace.change.post` | `WorkspaceRenamed` (`user_direct==true` 일 때만) |
| `tab.{create,delete}.post` | `TabCreated` / `TabClosed` |
| `tab.change.post` | `TabRenamed` (`user_direct==true` 일 때만) |
| `pane.{create,delete}.post` | `PaneCreated` / `PaneClosed` |
| `surface.{create,delete}.post` | `SurfaceCreated` / `SurfaceClosed` |

> `surface.change.post` 는 발화 site 없음(GUI 에서 surface 타입 직접 변경 경로 추가 시 등록).

### change.post 의 user-direct 분기

`PendingHostEvent::{WorkspaceRenamed, TabRenamed}` 가 `user_direct: bool` 을 들고 다닌다. rename dialog(사용자 직접 GUI)는 `true`, IPC 경유(`workspace.update`/`move`)는 `false`. plugin 이벤트 버스는 구분 없이 발화하되 **Lua hook 은 `user_direct==true` 일 때만** fire.

## Payload 스키마

모든 payload 타입은 `crates/tasty-plugin-protocol/src/events/payloads.rs`. Lua 측 필드 이름 = Rust 필드(snake_case). 핵심 필드:

| Payload | 주요 필드 |
|---------|-----------|
| `WindowCreated` | `window_id`, `kind`, `modality: "modeless"\|"modal"` |
| `WorkspaceCreated` | `workspace_id`, `window_id`, `name` |
| `WorkspaceRenamed` | `workspace_id`, `name?`, `subtitle?`, `description?` |
| `TabCreated` | `tab_id`, `pane_id`, `workspace_id`, `kind` |
| `PaneCreated` | `pane_id`, `parent_pane_group?`, `workspace_id` |
| `SurfaceCreated` | `surface_id`, `kind`, `tab_id`, `pane_id`, `workspace_id`, `created_by: { kind: "user"\|"agent", source_plugin? }` |
| `*Closed` | `<id>`, `reason` (+ kind) |

EmmyLua 자동완성: 스크립트 파일 옆 `.luarc.json` 에 `"workspace.library": ["<TASTY_REPO>/crates/tasty-lua/meta"]`.

## 새 이벤트 추가

1. `PendingHostEvent` variant 확인/추가 + 발화 site 배치(polling lifecycle detection 또는 imperative push).
2. `dispatch_pending_host_events` 매치 절에 plugin 버스 emit + `fire_lua` 호출.
3. [features/lua-hooks](../features/lua-hooks/index.md) 이벤트 표 · [design/policies/lua-hooks](../design/policies/lua-hooks.md) 매트릭스 · `crates/tasty-lua/meta/tasty.lua` stub 갱신.

## 새 호스트 API 추가

`crates/tasty-lua/src/host_api.rs::install` 에 `tasty.create_function(...)` 등록 후 `tasty` global table 에 set. 강한 sandbox 아님 — 사용자 머신·사용자 스크립트라 OS 권한이 충분, 별도 권한 체크 불필요.

## 에러 / 실행

- 콜백 Lua 에러 → `tracing::warn!` + 같은 이벤트 다음 콜백 계속(dispatch 안 멈춤). payload 직렬화 실패 → warn + 이 이벤트 콜백 전부 skip.
- 스크립트 실행 = 단축키 트리거(release) 또는 `debug.lua.eval`(debug). 워커 job 은 deadline 초과 시 abort(에러 반환) — 워커만 종료, 메인·다음 job 무영향. 부팅 자동로드(init.lua)·`script.reload` 는 ADR-0031 에서 제거됨.
- 디버그: `RUST_LOG=tasty_lua=debug`.
