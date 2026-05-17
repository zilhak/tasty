# Lua Hooks — 설계

사용자가 `~/.tasty/init.lua` 를 작성해 Tasty 의 특정 이벤트에 외부 자동화
함수를 붙일 수 있게 하는 시스템.

## 위치

| 항목 | 결정 |
|------|------|
| **사용 주체** | 호스트(Tasty 본체) 전용. plugin 은 Lua 를 사용하지 않는다. |
| **로딩 위치** | `~/.tasty/init.lua` 1 개. 모듈 import (`require`) 차단. |
| **권한** | observe-only — 콜백은 보고 외부 동작 (로그/알림/CLI 호출) 만 한다. 호스트 흐름을 바꾸거나 액션을 취소할 수 없다. |
| **샌드박스 강도** | 약 sandbox — `io`, `os.execute` 유지. 사용자 자신의 머신에서 자신이 작성한 스크립트이므로 권한 격리는 의미 없음. DoS / 무결성 보호 (memory cap, debug/loadlib/loadstring 제거) 만 적용. |

> Plugin 작성자가 Lua 를 쓰고 싶을 수 있겠지만, plugin 은 별 OS 프로세스로
> 격리되어 있고 Rust 만으로 충분히 표현 가능하므로 의도적으로 Lua 통로를
> 막았다. 만약 향후 plugin 측에 user-scripting hook 이 필요하면 별도 채널을
> 새로 만든다.

## 이벤트 매트릭스

엔티티 × 액션 × 페이즈 = `<entity>.<action>.<phase>` 형식. 1차 출시는 **post-only**.

| 엔티티 | create.post | delete.post | change.post |
|--------|:-:|:-:|:-:|
| tasty | startup.post | (없음) | (없음) |
| window | ✓ | ✓ | — |
| workspace | ✓ | ✓ | ✓ (rename) |
| tab | ✓ | ✓ | ✓ (rename) |
| pane | ✓ | ✓ | — |
| surface | ✓ | ✓ | (보류 — GUI 경로 부재) |

총 15 hook point.

### "change" 의 의미 — 사용자 직접 변경만

`change.post` 는 **사용자가 GUI 로 직접 변경한 경우** 에만 발화한다.

- workspace: rename dialog 로 title/subtitle/description 변경
- tab: rename dialog 로 title 변경
- surface: GUI 로 surface 타입 변경 (현재 그런 경로 없음 → 미발화)

IPC/CLI 경유 변경은 같은 wire 이벤트 (`workspace.renamed`, `tab.renamed`) 로
plugin 버스에는 발화되지만 Lua hook 으로는 가지 않는다. 자동 변경 (focus,
resize 등) 도 Lua change 이벤트 대상 아님.

이유: 사용자가 init.lua 에 적는 hook 은 "내가 직접 뭔가 바꿨을 때 외부 시스템에
알려주는" 용도가 대부분이다. IPC 자동화로 인한 변경까지 받으면 Slack 등에
중복 알림이 갈 수 있다.

구현은 `PendingHostEvent::{WorkspaceRenamed, TabRenamed}` 에 `user_direct: bool`
플래그로 구분한다. GUI dialog (`src/ui/dialog.rs::apply_rename`) 는 true, IPC
handler (`src/ipc/handler/workspace.rs`) 는 false.

## 왜 post-only

현재 권한이 observe-only 이므로 pre / post 의 의미 차이가 없다. pre 는
intervention (cancel / transform) 권한이 필요해질 때 의미를 갖는다. 그 시점에
pre 만 imperative call site 에 정밀 삽입하고 콜백 리턴값으로 흐름을 분기시킨다.

`tasty.shutdown` 도 1차 미노출. 프로세스 종료 시 hook 을 부르려면 별도
imperative fire 가 필요하고 (polling 으로는 안 됨), 1차 출시 범위 밖이다.

## 콜백 모델

- 등록: `tasty.on(event_name, callback)`. 동일 event 에 여러 콜백 등록 가능, 등록
  순서대로 호출.
- 호출 인자: 단일 table (event 별 payload 스키마). `docs/dev-guide/lua-hooks.md`
  참조.
- 콜백 에러: `tracing::warn!` 로 기록하고 같은 event 의 다음 콜백을 계속 진행.
  한 ill-behaved hook 이 전체 dispatch 를 막지 않는다.
- 리턴값: 무시. observe-only.

## 호스트 API

| 함수 | 설명 |
|------|------|
| `tasty.on(event, callback)` | hook 등록 |
| `tasty.log(msg)` | `tracing::info!` 로 로그 |
| `tasty.warn(msg)` | `tracing::warn!` 로 로그 |
| `tasty.notify(title, body)` | OS 알림 발사 (`notify-rust`) |
| `tasty.run_cli(args)` | `tasty` CLI 자체를 자식 프로세스로 spawn — 다른 자동화 도구와 동일한 표면 사용 |

추가 API 가 필요하면 dev-guide 의 매핑 표를 보고 host 측에 메서드를 추가한다.

## 향후 확장 시나리오

| 항목 | 트리거 |
|------|--------|
| `pre.*` 이벤트 | intervention 권한 도입 (cancel / transform) |
| `tasty.shutdown.post` | shutdown 시 hook 발화 인프라 추가 (현재 polling 으로는 부족) |
| plugin manifest 의 Lua 사용 | plugin 측에서도 user-scripting 이 필요한 use case 출현 시 — 현재 미계획 |
| surface.change.post | GUI 에서 surface 타입을 바꾸는 사용자 경로 추가 시 |
