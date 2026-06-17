# Lua 훅 (사용자 init.lua)

- **Status**: Implemented
- **주체**: 로컬 사용자 (자기 머신의 자기 스크립트)
- **ADR**: 없음 (설계 배경 `design/policies/lua-hooks` *재작성 예정*)
- **코드**: `tasty-lua` 크레이트(engine/host_api/sandbox), `script.reload` IPC
- **화면**: 없음

## 목적

시작 시 `~/.tasty/init.lua` 를 자동 로드. `tasty.on(event, callback)` 으로 이벤트 콜백을 등록해, **사용자가 GUI 를 조작했을 때** 외부 자동화(로그/알림/CLI 호출)를 트리거한다.

> **observe-only.** 콜백은 호스트 흐름을 바꿀 수 없고(cancel 불가) 이벤트는 모두 사후(post) 발화다. 이건 plugin Event Bus([reference/event-catalog](../../reference/event-catalog.md))와 별개의 *사용자 로컬* 자동화 경로다.

## 내부 동작

### 이벤트

`tasty.startup.post` + window/workspace/tab/pane/surface 의 `create.post`/`delete.post` + workspace·tab 의 `change.post`. 대부분 `workspace_id`/`tab_id`/`pane_id`/`surface_id` 를 payload 로 가진다. payload 스키마는 `crates/tasty-lua/meta/tasty.lua`(EmmyLua stub)가 정답.

> `change` 이벤트는 **사용자가 GUI 다이얼로그로 직접 바꾼 경우만** 발화한다 — IPC/CLI rename 은 발화 안 함(자동화가 이름 바꿀 때 hook 까지 도는 노이즈 방지).

### 호스트 API

`tasty.on(event, cb)` · `tasty.log/warn(msg)`(tracing) · `tasty.notify(title, body)`(OS 알림) · `tasty.run_cli({args})`(detached). `os.execute`/`io.*` 도 사용 가능 — 사용자 자신의 스크립트라 권한 격리 안 함.

### 안전 장치

메모리 32MB cap · 텍스트 청크만(bytecode 거부) · `debug`/`load*`/`dofile`/`package.loadlib` 제거 · 한 콜백 에러는 warn 기록 후 같은 이벤트의 다음 콜백 계속.

### 재로딩

`tasty script reload`(IPC `script.reload`) → init.lua 재실행, 기존 등록 전부 제거 후 새 등록만. 파일 없으면 `loaded:false`(에러 아님, hook 만 비워짐).

## 인터페이스

- **사용자**: `~/.tasty/init.lua` 편집 + `tasty script reload`. 디버깅은 `RUST_LOG=tasty_lua=debug`.

## 관련

- [reference/event-catalog](../../reference/event-catalog.md) — plugin 용 Event Bus(별개 경로)
- `design/policies/lua-hooks` *(재작성 예정)* — observe-only 설계 배경 · `dev-guide/lua-hooks` *(재작성 예정)* — payload 매핑
