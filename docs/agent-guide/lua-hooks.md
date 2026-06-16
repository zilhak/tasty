# Lua Hooks — 사용자 init.lua

Tasty 는 시작 시 `~/.tasty/init.lua` 를 자동으로 로드한다. 이 파일에서
`tasty.on(event, callback)` 으로 이벤트 콜백을 등록하면, 사용자가 직접
GUI 를 조작했을 때 외부 자동화 (로그/알림/CLI 호출) 를 트리거할 수 있다.

> **observe-only 입니다.** 콜백은 호스트의 흐름을 바꿀 수 없고 (cancel 불가),
> 이벤트는 모두 사후(post) 발화입니다. 더 자세한 설계 배경은
> `docs/design/policies/lua-hooks.md` 참조.

## 빠른 시작

```lua
-- ~/.tasty/init.lua

tasty.log("init.lua loaded")

-- workspace 가 새로 생기면 OS 알림
tasty.on("workspace.create.post", function(ctx)
  tasty.notify("New workspace", "id=" .. tostring(ctx.workspace_id))
end)

-- 사용자가 workspace title 을 GUI 로 바꾸면 Slack 으로 메시지
tasty.on("workspace.change.post", function(ctx)
  if ctx.name then
    os.execute(
      'curl -s -X POST -H "Content-type: application/json" '
      .. "--data '" .. string.format('{"text":"workspace renamed: %s"}', ctx.name)
      .. "' https://hooks.slack.com/services/..."
    )
  end
end)
```

`tasty script reload` (또는 IPC `script.reload`) 로 init.lua 를 재로딩한다.
재로딩 시 기존 등록은 모두 제거되고 새 등록만 살아남는다.

```sh
# init.lua 를 수정한 뒤
tasty script reload
# → { "loaded": true }   ← 파일이 존재하면 true, 없으면 false
```

`init.lua` 가 사라진 상태로 reload 해도 에러는 아니다 — 그냥 `loaded: false` 가
나오고 hook 등록만 비워진다.

## 이벤트 목록

총 15 개. 모두 사용자가 GUI 로 조작했을 때 또는 polling 이 변화를 감지했을 때
발화한다 (자동 변경/IPC 변경은 일부 제외 — 아래 참고).

| 이벤트 | 발화 시점 |
|--------|-----------|
| `tasty.startup.post` | Tasty 가 메인 윈도우 attach 직후 1 회 |
| `window.create.post` | Modeless window 가 새로 attach 됨 |
| `window.delete.post` | window 가 닫힘 |
| `workspace.create.post` | workspace 가 새로 생성됨 |
| `workspace.delete.post` | workspace 가 닫힘 |
| `workspace.change.post` | 사용자가 rename dialog 로 title/subtitle/description 변경 (**GUI 한정**) |
| `tab.create.post` | tab 이 새로 생성됨 |
| `tab.delete.post` | tab 이 닫힘 |
| `tab.change.post` | 사용자가 rename dialog 로 tab 이름 변경 (**GUI 한정**) |
| `pane.create.post` | pane 이 새로 생성됨 |
| `pane.delete.post` | pane 이 닫힘 |
| `surface.create.post` | surface 가 새로 생성됨 |
| `surface.delete.post` | surface 가 닫힘 |

> `change` 이벤트는 **사용자가 GUI 다이얼로그로 직접 바꾼 경우** 에만 발화한다.
> IPC/CLI 로 rename 한 경우는 발화되지 않는다 (자동화 도구가 이름을 바꾼다고
> Lua hook 까지 같이 도는 건 노이즈가 크기 때문).

### Payload 스키마

각 이벤트의 인자 (Lua table) 필드는 `docs/dev-guide/lua-hooks.md` 의 매핑 표 또는
`crates/tasty-lua/meta/tasty.lua` (EmmyLua stub) 를 참조한다.

대부분의 이벤트는 `workspace_id` / `tab_id` / `pane_id` / `surface_id` 같은 ID
필드를 포함한다.

## 호스트 API

| 함수 | 설명 |
|------|------|
| `tasty.on(event, callback)` | hook 등록. 동일 event 에 여러 콜백 OK. |
| `tasty.log(msg)` | `tracing::info!` 로 출력 (`RUST_LOG=tasty=info` 로 노출). |
| `tasty.warn(msg)` | `tracing::warn!` 로 출력. |
| `tasty.notify(title, body)` | OS 네이티브 알림 발사. |
| `tasty.run_cli({"arg1", ...})` | `tasty` CLI 를 자식 프로세스로 detached 실행. 표준 입출력은 null. |

`os.execute`, `io.*` 도 그대로 쓸 수 있다. 사용자 자신의 머신에서 사용자 자신의
스크립트가 도는 것이므로 권한 격리는 하지 않는다.

## 안전 장치

- 메모리 한계: 32 MB (초과 시 콜백 실패).
- 텍스트 청크만 로딩 — Lua bytecode 파일은 거부.
- `debug`, `package.loadlib`, `loadstring`, `loadfile`, `dofile`, `load` 제거.
- 한 콜백이 에러를 던지면 `warn` 로 기록만 하고 같은 이벤트의 다음 콜백을 계속 호출.

## 디버깅 팁

- `RUST_LOG=tasty_lua=debug tasty` 로 실행하면 hook 등록·발화 로그를 볼 수 있다.
- `tasty.log("...")` 출력은 `tasty -- --log-file ~/.tasty/tasty.log` 형식으로
  파일에 떨굴 수도 있다 (Tasty 의 일반 로깅 옵션 그대로).
- init.lua 파싱 실패 시 startup 로그에 에러가 찍히고, hook 은 모두 비등록 상태로
  시작한다.

## 예제 모음

### 새 workspace 생성 시 자동으로 디렉터리 cd

```lua
tasty.on("workspace.create.post", function(ctx)
  -- ctx.name 으로 workspace 이름 식별. 특정 이름에 대해서만 자동화 적용.
  if ctx.name == "scratch" then
    tasty.run_cli({"send", "text", "cd /tmp\r"})
  end
end)
```

### surface 가 닫힐 때 로그 파일에 기록

```lua
tasty.on("surface.delete.post", function(ctx)
  local f = io.open(os.getenv("HOME") .. "/.tasty/surface-deaths.log", "a")
  if f then
    f:write(os.date("%Y-%m-%d %H:%M:%S "))
    f:write("surface=" .. tostring(ctx.surface_id) .. "\n")
    f:close()
  end
end)
```

### tab 이름이 특정 패턴이면 알림

```lua
tasty.on("tab.change.post", function(ctx)
  if ctx.title:find("FAIL") then
    tasty.notify("Tab marked failing", ctx.title)
  end
end)
```
