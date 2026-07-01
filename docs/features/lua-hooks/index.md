# Lua 스크립트 (등록 + 단축키 트리거)

- **Status**: Implemented
- **주체**: 로컬 사용자 (자기 머신의 자기 스크립트)
- **ADR**: [0031](../../adr/0031-lua-host-api-only-worker-isolated.md) (설계 배경 [`design/policies/lua-hooks`](../../design/policies/lua-hooks.md))
- **코드**: `tasty-lua` 크레이트(engine/host_api/sandbox/bridge), 스크립트 저장소 `tasty-settings`(`ScriptRegistry`), 단축키 바인딩 `KeybindingSettings.script_bindings`
- **화면**: 설정 modal 단축키 탭 › Scripts (바인딩) + 기타(Misc) 탭 › Scripts (관리)

## 목적

사용자가 Lua 스크립트를 **등록**하고 **단축키에 연결**해 실행한다. 스크립트는 tasty 를 **열거된 고정 호스트 API 로만** 조작하며(첫 API `tasty.tree()`), 전용 워커 스레드에서 격리 실행된다. 부팅 시 임의 Lua 자동로드(`init.lua`)는 폐기됐다 — 스크립트는 명시 트리거로만 실행된다.

> **경계 = 호스트 API.** Lua 는 tasty 내부 state 에 직접 접근할 수 없다. 읽기는 메인이 발행한 스냅샷, 쓰기는 메인 커맨드 큐를 경유한다. 이벤트 hook `tasty.on` 콜백은 **observe-only** — 호스트 흐름을 바꿀 수 없다(plugin Event Bus([reference/event-catalog](../../reference/event-catalog.md))와 별개 경로).

## 내부 동작

### 등록 · 트리거

설정에 스크립트를 등록하면 `{id, name, path, sha256}` 가 config(`~/.tasty/config.toml`)에 영속된다(`ScriptRegistry`). 단축키 탭에서 스크립트에 combo 를 바인딩하고(`script_bindings`), 그 단축키를 누르면 워커에서 실행된다. **release 는 사용자 키 입력에서만** 이 경로를 탄다(identity 원칙 1). 임의 Lua 주입은 debug 빌드 전용(`debug.lua.eval`).

### 실행 격리 · 안전 장치

VM 은 전용 워커 스레드가 소유하고 job 을 직렬 처리한다. 메모리 32MB cap · 텍스트 청크만(bytecode 거부) · `debug`/`load*`/`dofile`/`package.loadlib` 제거 · 무한 루프/시간 초과는 instruction-count deadline 훅으로 abort(워커만 종료, 메인 무영향). `os.execute`/`io.*` 는 사용 가능 — 사용자 자신의 스크립트라 권한 격리 안 함.

### 무결성 (TOFU)

등록 시 엔트리 파일의 SHA256 을 기록한다. 단축키 발화 시 현재 파일 해시와 비교해, 변경됐으면 실행 전 **확인 popup** 을 띄운다(승인 시 해시 갱신 후 실행). transitive `require` 의존 파일은 커버하지 않는다.

### 호스트 API

`tasty.on(event, cb)` · `tasty.log/warn(msg)`(tracing) · `tasty.run_cli(args)`(메인 커맨드 큐 경유 detached spawn) · `tasty.tree()`(워크스페이스 트리 read, 스냅샷 경유). 표면은 열거된 것만 — 필요한 CRUD 마다 명시 등록으로 늘린다.

### 이벤트 hook

`tasty.on` / `fire` 배관은 유지되며 observe-only 다. 발화 이벤트: `tasty.startup.post` + window/workspace/tab/pane/surface 의 `create.post`/`delete.post` + workspace·tab 의 `change.post`. payload 스키마는 `crates/tasty-lua/meta/tasty.lua`(EmmyLua stub)가 정답.

> `change` 이벤트는 **사용자가 GUI 다이얼로그로 직접 바꾼 경우만** 발화한다 — IPC/CLI rename 은 발화 안 함. 부팅 자동로드가 폐기돼 현재는 hook 을 부팅에 자동 등록하는 경로가 없다 — 이벤트-트리거 자동실행 도입 시 등록 목록에서 배선한다.

## 인터페이스

- **사용자**: 설정 modal 에서 스크립트 등록·단축키 바인딩. 디버깅은 `RUST_LOG=tasty_lua=debug`.

## 관련

- [reference/event-catalog](../../reference/event-catalog.md) — plugin 용 Event Bus(별개 경로)
- [`design/policies/lua-hooks`](../../design/policies/lua-hooks.md) — 설계 배경 · [dev-guide/lua-hooks](../../dev-guide/lua-hooks.md) — payload 매핑 · [ADR-0031](../../adr/0031-lua-host-api-only-worker-isolated.md)
