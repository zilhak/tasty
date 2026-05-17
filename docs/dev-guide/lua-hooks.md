# Lua Hooks — 호스트 측 매핑

이 문서는 Tasty 호스트가 Lua hook 을 발화하는 코드 경로와 wire payload 스키마를
정리한다. 사용자 가이드는 `docs/agent-guide/lua-hooks.md`, 설계 배경은
`docs/design/lua-hooks.md` 를 참고.

## 구성

```
crates/tasty-lua/
  src/
    engine.rs     # LuaEngine — Lua VM + tasty.on dispatcher + fire()
    host_api.rs   # tasty.log / warn / notify / run_cli
    sandbox.rs    # 메모리 cap + 위험 글로벌 제거 + 텍스트-청크 모드
  meta/
    tasty.lua     # EmmyLua stub (LuaLS 용)
```

호스트 측 `App` 가 `lua_engine: Option<tasty_lua::LuaEngine>` 를 보유한다.
`main.rs::init_lua_engine` 이 `LuaEngine::new()` 후 `load_init(~/.tasty/init.lua)`
호출. init.lua 가 없으면 hook 없는 빈 엔진으로 정상 부팅.

이벤트 발화는 `fire_lua` 헬퍼 한 곳을 거친다:

```rust
pub(crate) fn fire_lua<T: Serialize>(
    lua: Option<&LuaEngine>,
    event: &str,
    payload: &T,
)
```

`payload` 는 `serde_json::Value` 로 직렬화된 뒤 Lua table 로 변환되어 콜백에
넘어간다. wire payload 스키마와 Lua table 필드는 1:1.

## 이벤트 ↔ 호스트 발화 site 매핑

| 이벤트 | 호스트 발화 위치 | Payload 타입 |
|--------|------------------|--------------|
| `tasty.startup.post` | `main.rs::main()` — `attach_main_window` 직후 | `serde_json::Value::Null` (인자 없음) |
| `window.create.post` | `App::attach_main_window` (`src/main.rs`) | `WindowCreated` |
| `window.delete.post` | `EventHandler` close 처리 (`src/event_handler.rs`) | `WindowClosed` |
| `workspace.create.post` | `dispatch_pending_host_events` → `WorkspaceCreated` | `WorkspaceCreated` |
| `workspace.delete.post` | `dispatch_pending_host_events` → `WorkspaceClosed` | `WorkspaceClosed` |
| `workspace.change.post` | `dispatch_pending_host_events` → `WorkspaceRenamed` (단 `user_direct==true` 일 때만) | `WorkspaceRenamed` |
| `tab.create.post` | `dispatch_pending_host_events` → `TabCreated` | `TabCreated` |
| `tab.delete.post` | `dispatch_pending_host_events` → `TabClosed` | `TabClosed` |
| `tab.change.post` | `dispatch_pending_host_events` → `TabRenamed` (단 `user_direct==true` 일 때만) | `TabRenamed` |
| `pane.create.post` | `dispatch_pending_host_events` → `PaneCreated` | `PaneCreated` |
| `pane.delete.post` | `dispatch_pending_host_events` → `PaneClosed` | `PaneClosed` |
| `surface.create.post` | `dispatch_pending_host_events` → `SurfaceCreated` | `SurfaceCreated` |
| `surface.delete.post` | `dispatch_pending_host_events` → `SurfaceClosed` | `SurfaceClosed` |

> `surface.change.post` 는 현재 발화 site 가 없다. GUI 에서 사용자가 surface
> 타입을 직접 바꾸는 경로가 추가되면 이 표에 함께 추가한다.

### change.post 의 user-direct 분기

`PendingHostEvent::{WorkspaceRenamed, TabRenamed}` 가 `user_direct: bool` 을
들고 다닌다.

| 호출 site | 값 | 이유 |
|-----------|----|------|
| `src/ui/dialog.rs::apply_rename` (rename dialog 확인) | `true` | 사용자 직접 GUI 입력 |
| `src/ipc/handler/workspace.rs` (workspace.update / workspace.move) | `false` | IPC 경유 자동화 |

`dispatch_pending_host_events` 는 plugin 이벤트 버스(`mgr.emit_host_event`) 는
구분 없이 발화하되, Lua hook 은 `user_direct==true` 일 때만 fire 한다.

## Payload 스키마

모든 payload 타입은 `crates/tasty-plugin-protocol/src/events/payloads.rs` 에
정의되어 있다. Lua 측에서 보이는 필드 이름은 Rust 필드 이름과 동일하다
(snake_case).

EmmyLua 자동완성/타입체크를 원하면 `crates/tasty-lua/meta/tasty.lua` 를
LuaLS 의 `Lua.workspace.library` 에 추가하라:

```jsonc
// .luarc.json (init.lua 옆에 두면 됨)
{
  "workspace.library": [
    "<TASTY_REPO>/crates/tasty-lua/meta"
  ]
}
```

핵심 payload 필드 요약 (자세한 건 EmmyLua stub 또는 payloads.rs 참조):

| Payload | 주요 필드 |
|---------|-----------|
| `WindowCreated` | `window_id: u64`, `kind: string`, `modality: "modeless"\|"modal"` |
| `WindowClosed` | `window_id: u64`, `reason: string` |
| `WorkspaceCreated` | `workspace_id: u32`, `window_id: u64`, `name: string` |
| `WorkspaceClosed` | `workspace_id: u32`, `reason: string` |
| `WorkspaceRenamed` | `workspace_id`, `name?: string`, `subtitle?: string`, `description?: string` |
| `TabCreated` | `tab_id`, `pane_id`, `workspace_id`, `kind: string` |
| `TabClosed` | `tab_id`, `pane_id`, `reason: string` |
| `TabRenamed` | `tab_id`, `title: string` |
| `PaneCreated` | `pane_id`, `parent_pane_group?: u32`, `workspace_id` |
| `PaneClosed` | `pane_id`, `reason: string` |
| `SurfaceCreated` | `surface_id`, `kind: string`, `tab_id`, `pane_id`, `workspace_id`, `created_by: { kind: "user" \| "agent", source_plugin?: string }` |
| `SurfaceClosed` | `surface_id`, `kind: string`, `reason: string` |

## 새 이벤트를 추가하려면

1. `PendingHostEvent` 에 variant 가 이미 있는지 확인. 없으면 추가 + 발화 site
   배치 (보통 polling lifecycle detection 또는 imperative push).
2. `dispatch_pending_host_events` 의 매치 절에 plugin 버스 emit + `fire_lua`
   호출을 둔다.
3. `docs/agent-guide/lua-hooks.md` 이벤트 표, `docs/design/lua-hooks.md` 매트릭스,
   `crates/tasty-lua/meta/tasty.lua` EmmyLua stub 셋 다 갱신.
4. `tasty-lua` 의 통합 테스트가 있다면 시나리오 추가.

## 새 호스트 API 를 추가하려면

`crates/tasty-lua/src/host_api.rs::install` 에 `tasty.create_function(...)` 으로
함수 등록 후 `tasty` global table 에 set. 강한 sandbox 가 아니므로 별도 권한
체크는 불필요 — 사용자 머신, 사용자 스크립트 이므로 OS 권한 = 충분.

## 콜백 에러 처리

- 콜백이 Lua 에러를 던지면 `tracing::warn!("lua hook for '{event}' failed: {e}")`
  로 기록 + 같은 이벤트의 다음 콜백 계속 호출. dispatch 전체가 멈추지 않는다.
- payload 직렬화 실패 (`lua.to_value(&ctx)`) 는 `warn` 로 기록하고 이 이벤트의
  콜백을 모두 skip — payload 가 깨지면 모든 콜백이 깨지는 게 정상이라고 본다.

## reload

`LuaEngine::reload()` 는 hook registry 를 비우고 같은 init.lua 를 다시 exec.
사용자가 `tasty script reload` CLI 또는 IPC `script.reload` 로 호출한다 (구현
위치는 본 가이드와 별개 — CLI/IPC dispatch 표에서 확인).

## 디버깅

- `RUST_LOG=tasty_lua=debug` 로 hook 등록·발화 로그.
- 모든 콜백을 강제로 빈 함수로 만들고 싶으면 `LuaEngine::reset_hooks()` 호출
  (debug-only IPC 로 노출 가능 — 현재 미배포).
