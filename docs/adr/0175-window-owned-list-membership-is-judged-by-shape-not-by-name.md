# ADR-0175: 창 소유 목록의 합산 여부는 이름이 아니라 성질로 판정한다

- **Status**: Accepted
- **Date**: 2026-09-05
- **Tags**: focus-independence, ipc, routing, list-aggregation

## Context

불가침 원칙 3 은 "`list` 는 전 워크스페이스 순회, 활성 상태 의존 동작 금지" 다. 창 소유
자원의 목록은 `src/app/dispatch/list_global.rs` 가 모든 main + parked engine 을 돌아
합쳐야 하고, **그 집합에 없는 목록은 포커스된 창의 것만 답한다 — 에러 없이.**

그 집합에 `tree` 가 빠져 있었다. 실측(2026-09-05, 격리 인스턴스, 창 둘, 창1 비포커스):

    tasty list panes       Pane 1 [ws:1]  ·  Pane 2 [ws:2]     ← 둘 다 보인다
    tasty list workspaces  Workspace(id:1) ·  Workspace(id:2)   ← 둘 다 보인다
    tasty list tree        Workspace(id:2) 만                   ← 창1 이 통째로 없다

`tree` 는 `handle_tree(state, engine, id)` 로 **engine 하나**만 받고 대상 인자가 없어
(`params = {}`) 라우터의 마지막 폴백(`id.or(focused_view_id)`)으로 떨어진다. 같은 빌더를
쓰는 Lua 스냅샷은 전 View + parked 를 합치고 있었고, 그 비대칭이 `build_engine_tree` 의
주석에 이미 적혀 있었다 — 결함으로 읽히지 않은 채로.

**왜 오래 안 보였나가 이 ADR 의 본체다.** 이 집합은 사람이든 census 든 **이름 모양**
(`*.list` / `*_list`)으로 훑여 왔다. 그 눈으로 메서드 표를 전수로 뽑으면 45 개가 나오는데
`tree` 는 거기 없다 — 이름에 `list` 가 없기 때문이다. 성질은 같은데 이름이 달라서, 훑는
쪽이 정확해도 대상 집합 자체가 그 항목을 안 담았다.

## Decision

**합산 대상인지는 이름이 아니라 세 성질로 판정한다**: ① 핸들러가 창 소유 컬렉션
(`engine.workspaces` 등)을 순회하는가 ② 요청이 대상을 지목할 인자가 없는가
③ `dispatch_list_global` 에 없는가. 셋을 모두 만족하면 그것은 이름과 무관하게 포커스
의존이고 합산 대상이다.

그 판정으로 `tree` 를 합산 집합에 넣는다. 워크스페이스 id 는 `IdGenerator` 공유 카운터로
창을 건너 유일하므로 이어 붙이면 그대로 키가 된다. 합친 목록에서 노드의 `active` 는
"자기 창에서 활성" 을 뜻해 창 수만큼 참이 될 수 있다 — 이미 합산 중인 `workspace.list`
가 같은 성질이므로 새로 생기는 뜻이 아니다.

판정을 사람의 눈에 맡기지 않기 위해 회귀 단언을 `multi_window_owner_routing` 에 둔다.
단언의 형태는 **수가 아니라 `workspace.list` 와의 집합 동등**이다 — 둘이 같은 물음에
답하므로 한쪽만 창을 건너면 그 자리에서 갈린다.

## Consequences

- **얻은 것**: `tasty list tree` 가 전 창을 답한다. 회귀는 짝이 되는 목록과의 집합 동등이
  잡는다 — 새 창 소유 목록이 `tree` 쪽에만 들어가도 갈린다.
- **잃은 것**: 없다. 종전 형태를 안 깨고 답의 범위만 넓혔다.
- **운영 비용**: 합산은 engine 수만큼의 순회다. 창 수는 사람이 여는 수라 상수에 가깝다.

## Alternatives Considered

- **A. `tree` 에 `window_id` 인자를 새로 받는다** — 표면 변경이고, 원칙이 요구하는 것은
  "지목할 수 있게" 가 아니라 "순회한다" 다. 지목 인자는 순회를 대신하지 못한다.
- **B. 이름 모양 판정을 넓힌다(`tree` 를 목록 이름 규칙에 편입)** — 이름을 고치면 CLI/IPC
  표면이 바뀌고, 다음에 또 이름이 다른 항목이 생기면 같은 자리로 돌아온다. 판정 기준을
  이름에서 떼는 것이 처방이다.
- **C. 가드로 세 성질을 정적으로 강제한다** — ①의 술어("창 소유 컬렉션을 순회하는가")가
  헬퍼 경유 읽기를 따라가야 하고, 그 술어는 이 저장소에서 이미 두 번 과대·과소 계상을
  냈다. 지금은 회귀 단언 하나로 두고, 아래 트리거에서 다시 본다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- 세 성질을 만족하는 항목이 **셋 이상** 새로 발견된다. 그때는 항목별 단언이 아니라 C 의
  정적 가드가 싸진다. 세는 명령은 [focus 정책](../design/policies/focus.md) 의 "재는 명령".
- 창 소유 자원의 id 가 창을 건너 유일하지 않게 된다 — 그러면 합산이 같은 id 를 둘 실어
  호출자가 어느 쪽도 지목 못 한다. 합산이 아니라 id 공간을 먼저 고쳐야 한다.
- `active` 가 창 수만큼 참이 되는 것이 소비자에게 실제 문제가 된다. 그때는 `tree` 만이
  아니라 `workspace.list` 와 함께 필드의 뜻을 바꾼다.

## References

- [focus 정책](../design/policies/focus.md) — "CLI/IPC 포커스 독립 원칙", "라우팅 아래에도 층이 하나 더 있다"
- [identity §2.3 포커스 독립성](../identity.md)
- `src/app/dispatch/list_global.rs` (합산 집합) · `src/adapters/ipc/handler.rs` (`build_engine_tree`)
- `tests/e2e_tests.rs` (`multi_window_owner_routing`) — 회귀 단언
