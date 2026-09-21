# ADR-0470: IPC 핸들러는 창 상태를 읽을 때만 `AppState` 를 받는다

- **Status**: Accepted
- **Date**: 2026-09-22
- **Tags**: ipc, handler, app-state, ownership, headless, signature, adr-0355, adr-0440

## Context

[ADR-0355](0355-app-state-ownership-is-split-by-the-gui-boundary-not-by-a-second-struct.md) 는
`AppState` 를 둘째 struct 로 가르지 않고 gui 경계로 갈랐다. 그 결정이 남긴 잔여 ① 은 "핸들러는
여전히 `AppState` 전체를 받는다 — '이 핸들러는 도메인 상태만 만진다' 를 타입이 말해 주지
않는다" 였다. 도메인 실행 쪽은 [ADR-0440](0440-the-domain-boundary-is-a-module-boundary-with-a-guard-not-a-crate.md)
의 포트 `CascadeWindow` 로 끝났고, 남은 것은 IPC 핸들러와 `pump_ipc` 였다.

실측(2026-09-22, 착수 트리 `81fd484b8`). 주석·문자열을 덮은 사본에서 `fn` 인자
`이름: &(mut )?AppState` 를 찾고 본문에서 그 이름이 어떻게 쓰이는지 셌다. 범위는
`src/adapters/ipc/**` · `src/boot/headless_dispatch.rs` · `src/app/ipc*` 다.

- 253 자리. 그중 **145 는 인자를 한 번도 안 읽었다**(이름이 `_state` 이거나 본문에 없다).
  55 는 통째로 다른 fn 에 넘기기만 했고, 53 은 필드·메서드를 직접 만졌다.
- 안 읽는 인자가 많은 이유는 라우터(`route_engine_handler`)가 모든 핸들러를 같은 모양
  `(core, state, engine, id, params)` 으로 부르던 관례다. 이미 일부는 그 모양을 벗어나
  있었다 — `workspace_category::handle_create(engine, …)`, `terminal::handle_children(engine, …)`.
- 직접 만지는 53 자리가 읽는 것: 로컬 사용자의 `active_workspace`(대상 생략 시 기본값) ·
  intent 큐(`dispatch_intent`) · GUI 전용 필드(`popups`·`banners`·`dialogs`·`modifier_hint`·
  `tool_registry` — debug·gui 모듈) · `ui.state` 덤프의 view 판정 · `recent_files` ·
  호스트 이벤트 큐. 그리고 `self` 를 안 읽는 `AppState` 메서드 둘(`read_output` ·
  `take_since_output_scan_mark`)과 `AppState` 가 든 Core memory 핸들 사본(`with_memory`).

## Decision

**핸들러의 인자는 그 핸들러가 닿는 상태를 말한다. 창 상태(`AppState`)를 읽지 않는 핸들러는
`AppState` 를 인자로 받지 않는다.** 둘째 struct 를 만들지 않고(ADR-0355 의 기각 그대로)
인자를 빼는 것으로 좁힌다. 그러면 "이 핸들러는 창 상태를 안 만진다" 를 컴파일러가 보증한다
— 본문이 창 상태를 읽으려면 시그니처부터 바꿔야 한다.

걸음은 셋이고 모두 동작을 바꾸지 않는다.

1. 안 읽는 인자를 뺀다. 넘기기만 하던 자리는 받는 쪽이 빠지면 안 읽는 자리가 되므로 고정점까지
   되풀이한다. 한 조합(gui 또는 headless)에서만 읽히는 인자는 남긴다.
2. `AppState` 가 든 Core memory 핸들 사본을 읽던 자리(`surface.meta.*` 넷과 `target_surface`
   의 nickname 해석, 그것을 쓰는 hard 점유 구조 가드)는 `Core` 로 읽는다. 두 운영 조립(GUI 창 ·
   headless 부팅)이 `AppState` 에 `Core::memory_arc` 의 같은 Arc 를 넘긴다.
3. `self` 를 안 읽는 `AppState` 메서드 둘을 `CoreState` 로 옮긴다(`core/state/output_read.rs`).
   포커스로 떨어지는 `read_since_mark` 는 로컬 사용자의 포커스를 읽으므로 `AppState` 에 남긴다.

**라우터와 `pump_ipc` 는 `AppState` 를 계속 받는다.** 남은 핸들러가 실제로 창 상태를 읽기
때문이다 — 라우터는 그 핸들러들에게 넘겨야 하고, `check_request` 자신도 발화 주체의 기본
워크스페이스를 `active_workspace` 에서 읽는다.

## Consequences

- **얻은 것**: IPC 범위에서 `AppState` 를 받는 자리가 253 → 91 이다(같은 판정기, 2026-09-22).
  안 읽는 자리 145 → 0. 남은 91 은 직접 읽는 46 과, 그것을 부르려고 넘기는 45 다. memory ·
  agent 협업 · telemetry 조회 · approval 조회 · surface 조회·전송 · hook · message ·
  terminal tell · 구조 list 류 핸들러는 이제 `Core`/`CoreState` 만 받는다.
- **얻은 것**: IPC 응답 형태·CLI 출력·plugin wire 가 하나도 안 바뀐다. 모든 걸음이 인자 제거
  또는 이동이다.
- **남은 것 — 창 상태를 읽는 핸들러** (91 자리의 내용). 이것들은 좁혀도 창 상태 쪽 인자가 남는다.
  - intent 큐에 넣기만 하는 핸들러(`surface.set_mark` · `surface.completion` ·
    `surface.attention.clear` · `notification.create` · `file_handler.dispatch` · `markdown.navigate` ·
    `settings.set_remote_transfer` 등). 큐가 창마다 있다.
  - 대상을 생략하면 로컬 사용자의 `active_workspace` 로 떨어지는 핸들러(`workspace.list` 의
    `focused` 칸, `system.info`, `tree`, approval 요청·telemetry 기록의 기본 워크스페이스, image ·
    webview 의 대상 해석). 포커스 독립성 정책상 "조회는 허용, 의존은 금지" 의 경계에 있는 자리들이다
    — 이 결정은 그 동작을 안 바꾼다.
  - GUI·debug 전용 모듈(`debug*`, `tool`, `file_picker`)과 `ui.state` 덤프.
  - 구조 op 중 창 연산을 부르는 것(`workspace.close` 의 `close_workspace_at` ·
    `workspace.create` 의 cwd 상속 · 구조 intent 를 거는 close·split·tab 경로).
- **잃은 것**: 라우터의 호출 모양이 한 가지가 아니다. 핸들러마다 인자 목록이 다르므로 새
  핸들러를 등록하는 사람은 그 핸들러가 무엇을 받는지 보고 부른다.
- **운영 비용**: 새 핸들러는 필요한 상태만 받는다. 창 상태가 필요 없는 핸들러에 `_state` 를
  달지 않는다 — 그렇게 달면 컴파일러가 아무 말도 안 하고 이 결정이 준 보증이 그 자리에서 사라진다.
  이것을 막는 가드는 두지 않았다(아래 재검토 조건).

## Alternatives Considered

- **`AppState` 를 도메인 struct 와 창 struct 로 가른다**: ADR-0355 가 기각한 안이다. 이번
  실측으로 그 필요가 더 줄었다 — 안 읽는 인자를 빼는 것만으로 IPC 범위의 절반 넘게가 창
  상태에서 떨어진다.
- **intent 큐만 쓰는 핸들러에 큐 하나(`&mut Vec<DispatchedIntent>`)만 넘긴다**: 시그니처가 "큐에
  넣기만 한다" 를 말하게 된다. 안 고른 이유: 큐는 창 소유이고 요청이 어느 창의 큐에 들어가는지가
  라우팅의 일이라, 큐를 따로 떼어 넘기는 모양이 창 상태를 받는 것과 소유 관계상 다르지 않다.
  그리고 큐 push 앞뒤에 창 상태를 읽는 핸들러가 섞여 있어 모양이 둘로 갈린다. 핸들러 12 자리의
  이득으로 규칙을 하나 더 두지 않았다.
- **`active_workspace` 를 읽는 핸들러의 대상 생략 갈래를 없앤다**: 포커스 독립성 쪽 물음이고
  외부 동작(대상 생략 시 응답)이 바뀐다. 이 결정은 인자의 모양만 다룬다.
- **`_state` 로 된 안 읽는 인자를 막는 가드를 둔다**: 한 번 없앤 형태가 다시 들어오는 입구를
  막는다. 이번에 안 둔 것은 판정기가 fn 인자의 사용 여부를 세야 해서 가드가 무거워지는 데 비해,
  입구가 새 핸들러 등록 한 자리뿐이기 때문이다.

## Reconsideration Triggers

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- 핸들러가 창 상태를 새로 읽어야 한다 — 그 핸들러의 시그니처에 `AppState` 가 없으면 컴파일이
  깨진다. 그때 인자를 더하는 것이 맞는지(창 상태인가) 도메인에 둘 것인지 정한다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다.

- 안 읽는 `AppState` 인자가 다시 늘어난다. 재는 법: 주석·문자열을 덮은 사본(`mask-source`)에서
  `fn` 인자 `이름: &(mut )?AppState` 중 이름이 `_` 로 시작하거나 본문에 안 나오는 것을 센다
  (이 결정 직후 IPC 범위 0). 그 수가 다시 쌓이면 가드를 둔다.
- 도메인 실행을 `AppState` 없이 도는 별도 소비자(크레이트 분리 · 별도 데몬)를 세우게 된다 —
  그때는 위 "남은 것" 의 넷째 묶음(구조 op 의 창 연산)이 `CascadeWindow` 처럼 포트로 가야 한다.
  재는 법: 같은 판정기로 IPC 범위의 access 자리를 모아 읽는 필드·메서드를 대조한다.

## References

- [ADR-0355](0355-app-state-ownership-is-split-by-the-gui-boundary-not-by-a-second-struct.md) — 이 결정이 닫는 잔여 ①
- [ADR-0440](0440-the-domain-boundary-is-a-module-boundary-with-a-guard-not-a-crate.md) — 도메인 쪽 포트
- [AppState 필드 소유권](../dev-guide/app-state-ownership.md) — 필드 분류표
- [포커스 정책](../design/policies/focus.md)
- 결정이 실현된 현재 위치: `route_engine_handler`(`src/adapters/ipc/handler.rs`) 의 핸들러 호출 모양 ·
  `CoreState::read_output` · `CoreState::take_since_output_scan_mark` · `pane::resolve_surface_target`
