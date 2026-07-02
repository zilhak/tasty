# Lua 훅 — 설계 정책

사용자가 등록한 Lua 스크립트로 tasty 를 조작·자동화하는 시스템의 *설계 근거*. 전체 결정은 [ADR-0031](../../adr/0031-lua-host-api-only-worker-isolated.md). 사용법은 [features/lua-hooks](../../features/lua-hooks/index.md), payload 매핑은 [dev-guide/lua-hooks](../../dev-guide/lua-hooks.md).

## 위치 결정 (ADR-0031)

| 항목 | 결정 |
|------|------|
| 사용 주체 | **호스트 전용.** plugin 은 Lua 미사용 |
| 등록·트리거 | 설정에 스크립트를 **등록**하고(SHA256 TOFU) **단축키 또는 이벤트 트리거(자동실행)로 실행**. 부팅 시 `~/.tasty/init.lua` 자동로드는 폐기 — 자동실행도 임의 로드가 아니라 등록 목록의 명시 트리거에서 배선. `require` 모듈 import 차단 |
| tasty 접근 | **열거된 고정 호스트 API 표면으로만.** state 직접 접근 불가, CRUD 전부 API. 첫 API 는 트리 조회 `tasty.tree()`(read) |
| 실행 격리 | **전용 워커 스레드.** 읽기=메인 발행 스냅샷, 쓰기=메인 커맨드 큐. 무한 루프/시간 초과는 instruction-count deadline 훅으로 abort |
| 무결성 | 등록 시 SHA256 기록 → **TOFU**. 수동 발화(단축키) 변경 시 확인 popup, 자동(이벤트) 발화 변경 시 **실행 차단 + `tracing::warn` + 관리창(Misc›Scripts) changed 배지**. 자동 경로는 popup/배너를 쓰지 않는다 — 발화에 사용자 계기가 없고, 배너는 사용자 직접 조작에서만 발사된다는 발화 정책과 충돌하기 때문. 해시 자동 갱신 금지(자동 승인은 TOFU 무의미) |
| 권한 | 이벤트 hook 콜백은 **observe-only** — 보고 외부 동작만. 명시 API 호출을 통한 active CRUD 는 직교 채널(흐름 소유권은 호스트) |
| 샌드박스 | 약 sandbox — `io`/`os.execute` 유지(자기 머신 자기 스크립트라 격리 무의미). 능력 제한 목적은 ① tasty 접근을 API 로 좁힘 ② 워커/메인 스레드 안전. DoS/무결성 보호(메모리 cap, `debug`/`loadlib`/`load*` 제거)만 |

> plugin 은 별 OS 프로세스로 격리돼 Rust 로 충분하므로 Lua 통로를 의도적으로 막았다. plugin 측 user-scripting 이 필요해지면 별도 채널을 새로 만든다(ADR-0009 와 함께 재검토).

## 이벤트 매트릭스 — post-only

`<entity>.<action>.<phase>` 형식, 1차는 **post-only**:

| 엔티티 | create.post | delete.post | change.post |
|--------|:-:|:-:|:-:|
| tasty | startup.post | — | — |
| window | ✓ | ✓ | — |
| workspace | ✓ | ✓ | ✓(rename) |
| tab | ✓ | ✓ | ✓(rename) |
| pane | ✓ | ✓ | — |
| surface | ✓ | ✓ | (보류 — GUI 경로 부재) |

### `change` = 사용자 직접 변경만

`change.post` 는 **사용자가 GUI 다이얼로그로 직접 바꾼 경우에만** 발화. IPC/CLI rename 은 plugin 버스 이벤트(`workspace.renamed` 등)로는 가지만 Lua hook 으론 안 간다 — 자동화로 인한 변경까지 받으면 Slack 등에 중복 알림. 구현은 `PendingHostEvent` 의 `user_direct: bool`(GUI dialog=true, IPC handler=false)로 구분.

## 왜 post-only

현재 권한이 observe-only 라 pre/post 의미 차이가 없다. pre 는 intervention(cancel/transform) 권한이 생길 때 의미를 갖고, 그때 imperative call site 에 정밀 삽입하고 콜백 리턴으로 분기시킨다. `tasty.shutdown` 도 1차 미노출(polling 으론 부족, imperative fire 인프라 필요).

## 콜백 모델

`tasty.on(event, cb)`(동일 event 다중 등록, 순서대로). 인자는 단일 table(payload). 콜백 에러는 `tracing::warn!` 기록 후 다음 콜백 계속(한 ill-behaved hook 이 전체 dispatch 막지 않음). 리턴값 무시(observe-only). 호스트 API 표면(현재): `tasty.on`/`log`/`warn`/`run_cli`(커맨드 큐 경유)/`tree`(read).

이벤트 hook `fire`/`tasty.on` 배관은 유지되지만, 부팅 자동로드(init.lua)가 폐기되어 **hook 을 부팅에 자동 등록하는 경로는 없다.** 이벤트-트리거 **자동실행은 별도(직교) 채널로 구현되어 있다** — 콜백을 깨우는 것이 아니라, 등록 목록(`ScriptEntry.triggers`)에 바인딩된 스크립트 **소스를 트리거 발화 시 TOFU 재검 후 실행**한다(ADR-0031 의 "등록 목록에서 배선" 요구 충족).

## 자동실행 (autofire)

- **트리거**: host 가 실제 fire 하는 lifecycle 이벤트 13종 화이트리스트(`AUTO_TRIGGER_EVENTS`, `crates/tasty-settings/src/scripts.rs`)만 등록 가능. 저장은 `ScriptEntry.triggers`(단축키 combo 는 계속 `KeybindingSettings` 소유 — 이벤트 트리거는 combo 충돌 개념이 없어 scripts 소유).
- **identity 정합**: 자동실행은 사용자가 config 에 직접 바인딩한 "사용자 설정 행동" — 에이전트 행동이 아니므로 release 에 존재한다(단축키 트리거와 동일 논리). 트리거 바인딩을 조작하는 IPC API 는 만들지 않는다(설정 UI/config 경유만).
- **cascade 방어**: 자동실행 스크립트가 `run_cli` 로 자기 트리거 대상을 만들면 재발화 연쇄가 생긴다. per-job deadline 은 1회 실행만 보므로, **재진입 가드**(`AutofireGuard`, `src/host_api/hooks/autofire.rs`)가 in-flight + 완료 직후 1 프레임 동안 신규 자동실행을 전역 억제해 연쇄를 유한하게 끊는다. origin(user/agent) 게이트는 미배선 — create 계열 이벤트에 origin 판별자가 없어(§ 아래) 게이트에 의존할 수 없다.

## 향후 확장

`pre.*`(intervention 권한 도입 시) · `tasty.shutdown.post`(shutdown fire 인프라) · surface `change.post`(GUI 타입 변경 경로 추가 시) · 호스트 API 표면 확대(mutation CRUD) · plugin Lua(미계획).

## 관련

- [features/lua-hooks](../../features/lua-hooks/index.md) — 사용법·API · [reference/event-catalog](../../reference/event-catalog.md) — plugin 용 Event Bus(별개)
