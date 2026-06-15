# Lua Hooks (사용자 init.lua)

- **Status**: Implemented

호스트 전용 user scripting 레이어. 사용자가 `~/.tasty/init.lua` 에 `tasty.on("<event>", function(ctx) ... end)` 를 적어 GUI 동작에 외부 자동화를 붙일 수 있다. observe-only — 콜백은 호스트 흐름을 바꿀 수 없다. Plugin 은 Lua 를 사용하지 않는다 (Rust 전용).

### 엔진
- `tasty-lua` 크레이트 — mlua 0.10 (Lua 5.4, vendored)
- 단일 `LuaEngine` 인스턴스가 `App` 에 보유 — 메인 스레드 1 군데서만 호출
- 약 sandbox: 메모리 32 MB cap, 텍스트-청크만 (bytecode 거부), `debug`/`loadstring`/`loadfile`/`dofile`/`load`/`package.loadlib` 제거. `io`/`os.execute` 는 유지 (사용자 자기 머신/자기 스크립트)

### 호스트 API
- `tasty.on(event, callback)` — hook 등록
- `tasty.log(msg)` / `tasty.warn(msg)` — `tracing::info!` / `warn!`
- `tasty.notify(title, body)` — OS 네이티브 알림 (notify-rust)
- `tasty.run_cli(args)` — `tasty` CLI 를 자식 프로세스 detached 실행

### 이벤트 (15 hook point, post-only)
- `tasty.startup.post`
- `window.create.post` / `window.delete.post`
- `workspace.create.post` / `workspace.delete.post` / `workspace.change.post`
- `tab.create.post` / `tab.delete.post` / `tab.change.post`
- `pane.create.post` / `pane.delete.post`
- `surface.create.post` / `surface.delete.post`
- `change.post` 는 **사용자가 GUI 다이얼로그로 직접 변경** 한 경우만 발화. IPC/CLI 경유 변경은 plugin 이벤트 버스에는 가지만 Lua hook 으로는 안 감 (`PendingHostEvent::{WorkspaceRenamed,TabRenamed}` 에 `user_direct: bool` 플래그로 분기)

### 콜백 isolation
- 콜백 에러는 `tracing::warn!` 로 기록 + 같은 이벤트의 다음 콜백을 계속 호출
- payload 직렬화 실패 시 이 이벤트의 모든 콜백을 skip
- 콜백 리턴값은 무시 (observe-only)

### EmmyLua stub
- `crates/tasty-lua/meta/tasty.lua` — LuaLS 의 `workspace.library` 에 추가하면 자동완성/타입체크 가능

### Reload
- IPC: `script.reload` (local_only, plugin 호출 불가) — `{ loaded: bool }` 반환
- CLI: `tasty script reload`
- 재로딩 시 기존 등록 hook 모두 제거 후 같은 init.lua 재실행

### 한계 (현재)
- pre 이벤트 없음 — observe-only 에선 의미 없음. intervention 권한 도입 시 추가
- `tasty.shutdown.post` 없음 — shutdown 시 fire 인프라 별도 필요
- `surface.change.post` 발화 site 없음 — GUI 에서 surface 타입 변경하는 경로 부재
