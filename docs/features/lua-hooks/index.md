# Lua 스크립트 (등록 + 단축키/이벤트 자동실행 트리거)

- **Status**: Implemented
- **주체**: 로컬 사용자 (자기 머신의 자기 스크립트)
- **ADR**: [0031](../../adr/0031-lua-host-api-only-worker-isolated.md) (설계 배경 [`design/policies/lua-hooks`](../../design/policies/lua-hooks.md))
- **코드**: `tasty-lua` 크레이트(engine/host_api/sandbox/bridge), 스크립트 저장소 `tasty-settings`(`ScriptRegistry`), 단축키 바인딩 `KeybindingSettings.script_bindings`
- **화면**: 설정 modal 단축키 탭 › Scripts (단축키 바인딩) + 기타(Misc) 탭 › Scripts (관리 + 자동실행 트리거 편집)

## 목적

사용자가 Lua 스크립트를 **등록**하고 **단축키에 연결**하거나 **lifecycle 이벤트 트리거에 바인딩(자동실행)**해 실행한다. 스크립트는 tasty 를 **열거된 고정 호스트 API 로만** 조작하며(첫 API `tasty.tree()`), 전용 워커 스레드에서 격리 실행된다. 부팅 시 임의 Lua 자동로드(`init.lua`)는 폐기됐다 — 스크립트는 명시 트리거로만 실행된다.

> **경계 = 호스트 API.** Lua 는 tasty 내부 state 에 직접 접근할 수 없다. 읽기는 메인이 발행한 스냅샷, 쓰기는 메인 커맨드 큐를 경유한다. 이벤트 hook `tasty.on` 콜백은 **observe-only** — 호스트 흐름을 바꿀 수 없다(plugin Event Bus([reference/event-catalog](../../reference/event-catalog.md))와 별개 경로).

## 내부 동작

### 등록 · 트리거

설정에 스크립트를 등록하면 `{id, name, path, sha256, triggers}` 가 config(`~/.tasty/config.toml`)에 영속된다(`ScriptRegistry`). 트리거 채널은 둘:

- **단축키** — 단축키 탭에서 combo 바인딩(`script_bindings`, `KeybindingSettings` 소유). 누르면 워커에서 실행.
- **이벤트 자동실행** — 관리 창에서 lifecycle 이벤트를 트리거로 추가(`ScriptEntry.triggers`). host 가 그 이벤트를 fire 할 때 TOFU 재검 후 자동 실행된다. 등록 가능 이벤트는 host 가 실제 fire 하는 13종 화이트리스트(`AUTO_TRIGGER_EVENTS`): `tasty.startup.post` + window/workspace/tab/pane/surface 의 `create.post`/`delete.post` + workspace·tab 의 `change.post`. **release 는 사용자 키 입력에서만** 이 경로를 탄다(identity 원칙 1). 임의 Lua 주입은 debug 빌드 전용(`debug.lua.eval`).

**관리 창** — 설정 modal 기타(Misc) 탭 › **Scripts**(전 플랫폼·최상단, `src/view/settings/ui/tabs/misc.rs::draw_scripts_subtab`). 등록 스크립트를 행 목록으로 보여준다: script 글리프 · 표시 이름 · 중간생략 경로(디렉토리 tail 이 먼저 ellipsis, 파일명은 완전 표시) · 바운드 단축키 `Kbd` 또는 "Unbound". 행 액션은 **bind**(→ Keybindings › Scripts 진입만; 바인딩 편집은 단축키 탭 소유), **rename**(인라인), **remove**(인라인 확인 + 연결 단축키 자동 해제). 각 행 하단에 **자동실행 트리거** 행이 있다 — 등록된 트리거는 chip(이벤트명 + ✕, 클릭=제거)으로 표시하고, 화이트리스트 중 미등록 이벤트만 노출하는 콤보박스로 추가한다. 인라인 Add card(File + Browse… `.lua` 필터 / Display name)로 등록하며, 창을 열 때 디스크 해시를 저장 해시와 비교해 불일치 시 **changed** 배지 + 안내(TOFU 재확인 예고)를 표시한다. 등록이 없으면 빈 상태를 그린다.

### 실행 격리 · 안전 장치

VM 은 전용 워커 스레드가 소유하고 job 을 직렬 처리한다. 메모리 32MB cap · 텍스트 청크만(bytecode 거부) · `debug`/`load*`/`dofile`/`package.loadlib` 제거 · 무한 루프/시간 초과는 instruction-count deadline 훅으로 abort(워커만 종료, 메인 무영향) — 자동실행 경로도 동일 `Run` job 이라 deadline 이 그대로 적용된다. `os.execute`/`io.*` 는 사용 가능 — 사용자 자신의 스크립트라 권한 격리 안 함.

**자동실행 재진입 가드** — 자동실행 스크립트가 `tasty.run_cli` 로 자기 트리거 대상을 만들면(예: `surface.create.post` 바인딩 스크립트가 split 실행) 재발화 연쇄가 생긴다. deadline 은 1회 실행만 보므로, `AutofireGuard`(`src/host_api/hooks/autofire.rs`)가 자동실행 in-flight + 완료 직후 1 프레임 동안 신규 자동실행을 전역 억제해 연쇄를 유한하게 끊는다(억제 시 `tracing::warn`).

### 무결성 (TOFU)

등록 시 엔트리 파일의 SHA256 을 기록한다. 매 발화(단축키·자동실행)마다 현재 파일 해시와 비교한다:

- **단축키(수동)**: 불일치 시 실행 전 **확인 popup**(승인 시 해시 갱신 후 실행). 사용자가 계기이므로 popup 이 정당.
- **자동실행**: 불일치 시 **실행 차단 + `tracing::warn`** — 관리 창의 changed 배지로 확인하고 재승인해야 한다. 사용자 개입 없이 발화하므로 popup/배너를 띄우지 않고, 해시도 자동 갱신하지 않는다(자동 승인은 TOFU 무의미).

transitive `require` 의존 파일은 커버하지 않는다.

### 호스트 API

`tasty.on(event, cb)` · `tasty.log/warn(msg)`(tracing) · `tasty.run_cli(args)`(메인 커맨드 큐 경유 detached spawn) · `tasty.tree()`(워크스페이스 트리 read, 스냅샷 경유). 표면은 열거된 것만 — 필요한 CRUD 마다 명시 등록으로 늘린다.

### 이벤트 hook

`tasty.on` / `fire` 배관은 유지되며 observe-only 다. 발화 이벤트: `tasty.startup.post` + window/workspace/tab/pane/surface 의 `create.post`/`delete.post` + workspace·tab 의 `change.post`. payload 스키마는 `crates/tasty-lua/meta/tasty.lua`(EmmyLua stub)가 정답.

> `change` 이벤트는 **사용자가 GUI 다이얼로그로 직접 바꾼 경우만** 발화한다 — IPC/CLI rename 은 발화 안 함. 부팅 자동로드가 폐기돼 hook 을 부팅에 자동 등록하는 경로는 없다. 이벤트-트리거 **자동실행**은 이 observe-hook 과 직교하는 별도 채널이다 — 콜백을 깨우는 게 아니라 같은 fire 지점에서 등록 목록의 바인딩 스크립트를 실행한다.

## 인터페이스

- **사용자**: 설정 modal 에서 스크립트 등록·단축키 바인딩. 디버깅은 `TASTY_LOG=tasty_lua=debug`.

## 관련

- [reference/event-catalog](../../reference/event-catalog.md) — plugin 용 Event Bus(별개 경로)
- [`design/policies/lua-hooks`](../../design/policies/lua-hooks.md) — 설계 배경 · [dev-guide/lua-hooks](../../dev-guide/lua-hooks.md) — payload 매핑 · [ADR-0031](../../adr/0031-lua-host-api-only-worker-isolated.md)
