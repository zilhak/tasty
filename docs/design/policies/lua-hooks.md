# Lua 훅 — 설계 정책

사용자가 `~/.tasty/init.lua` 로 tasty 이벤트에 외부 자동화를 붙이는 시스템의 *설계 근거*. 사용법은 [features/lua-hooks](../../features/lua-hooks/index.md), payload 매핑은 `dev-guide/lua-hooks` *(재작성 예정)*.

## 위치 결정

| 항목 | 결정 |
|------|------|
| 사용 주체 | **호스트 전용.** plugin 은 Lua 미사용 |
| 로딩 | `~/.tasty/init.lua` 1개. `require` 모듈 import 차단 |
| 권한 | **observe-only** — 콜백은 보고 외부 동작(로그/알림/CLI)만. 호스트 흐름 변경·액션 취소 불가 |
| 샌드박스 | 약 sandbox — `io`/`os.execute` 유지(자기 머신 자기 스크립트라 격리 무의미). DoS/무결성 보호(메모리 cap, `debug`/`loadlib`/`load*` 제거)만 |

> plugin 은 별 OS 프로세스로 격리돼 Rust 로 충분하므로 Lua 통로를 의도적으로 막았다. plugin 측 user-scripting 이 필요해지면 별도 채널을 새로 만든다.

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

`tasty.on(event, cb)`(동일 event 다중 등록, 순서대로). 인자는 단일 table(payload). 콜백 에러는 `tracing::warn!` 기록 후 다음 콜백 계속(한 ill-behaved hook 이 전체 dispatch 막지 않음). 리턴값 무시(observe-only). 호스트 API: `tasty.on`/`log`/`warn`/`notify`/`run_cli`.

## 향후 확장

`pre.*`(intervention 권한 도입 시) · `tasty.shutdown.post`(shutdown fire 인프라) · surface `change.post`(GUI 타입 변경 경로 추가 시) · plugin Lua(미계획).

## 관련

- [features/lua-hooks](../../features/lua-hooks/index.md) — 사용법·API · [reference/event-catalog](../../reference/event-catalog.md) — plugin 용 Event Bus(별개)
