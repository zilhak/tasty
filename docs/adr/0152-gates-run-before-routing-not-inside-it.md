# ADR-0152: 게이트는 라우팅보다 먼저 돈다 — 조기 응답이 검사 자리를 건너뛴다

- **Status**: Accepted
- **Date**: 2026-09-05
- **Tags**: security, permissions, ipc, plugin, routing, guards, telemetry, audit

## Context

권한 게이트가 라우터 **안쪽**(`handle_with_caller`)에만 있었다. 그런데 GUI 앱의
`App::dispatch_with_caller` 는 거기 도달하기 전에 끝나는 경로를 여럿 갖는다 —
`dispatch_list_global` 의 list 합산 응답, owner 해석 실패, 지목한 대상 없음, app state
없음. 그중 첫째가 `surface.list` · `workspace.list` · `pane.list` 를 게이트 앞에서 답했다.

셋 다 `plugin(&[SurfaceRead])` 로 선언돼 있고 [plugin-permissions](../dev-guide/plugin-permissions.md)
의 `surface.read` 행이 그것들을 싣는다. 표와 문서는 서로 맞았고 **집행만 안 됐다.** 표가
이름 수준에서 일치한다는 것과 그 권한이 실제로 걸린다는 것은 다른 명제다.

**진단을 확정한 것은 같은 회차 안의 대조다.** 권한을 하나도 선언하지 않은 번들 plugin 이
다섯 개를 연달아 불렀을 때, 같은 `surface.read` 를 요구하는 두 메서드가 서로 다르게
답했다.

    surface.list       -> OK   전체 surface 트리
    workspace.list     -> OK   전체 workspace 목록
    pane.list          -> OK   전체 pane 목록
    tab.list           -> ERR  permission_denied: missing 'surface.read'
    clipboard.set_text -> ERR  permission_denied: missing 'clipboard.write'

`tab.list` 은 **`surface.list` 과 같은 권한**을 요구하면서 거부됐다. 검사 로직이 없거나
망가졌다면 둘 다 통과했어야 한다. 하나만 새는 것은 **게이트가 없었다는 뜻이 아니라 게이트가
있는데 자리가 틀렸다는 뜻**이고, 결정이 아래 A 가 아니라 본 결정인 이유가 여기서 나온다.

**노출된 것**: 전체 workspace · pane · surface 트리에 각 surface 의 `foreground_pid`,
전경 프로세스 이름, 치수가 들어 있다. 권한 선언이 빈 plugin 이 사용자가 무엇을 어디서
돌리고 있는지를 읽을 수 있었다. 정보 노출이다.

건너뛰는 것은 권한만이 아니었다. 같은 자리에서 telemetry cap 과 rate limit 도 넘어가고
audit 기록도 남지 않았다 — 그 회차의 audit 은 거부된 둘만 1 행씩 갖고 샌 셋은 0 행이었다.
cap 이 Pause 로 모든 IPC 를 막은 plugin 도 이 셋은 통과한다는 뜻이다.

헤드리스에는 이 구멍이 없다. `headless_dispatch.rs` · `headless_plugins.rs` 는
`handle_with_caller` 직결이라 게이트를 탄다. GUI 앱 경로 전용이었다.

## Decision

**게이트 3종(권한 / telemetry cap / rate limit)을 `App::dispatch_with_caller` 진입부로
올린다.** 라우팅보다 먼저 돌므로, 그 아래에서 어떤 형태로 조기에 응답하든 검사를
건너뛸 수 없다.

라우터 안쪽 게이트는 **남긴다.** `handle_with_caller` 를 직접 부르는 진입점(headless ·
attach · routing)이 따로 있어 그쪽은 안쪽 게이트가 지킨다. 중복이 아니라 **경계가 둘**
이고, 거부는 바깥에서 단락되므로 안쪽이 다시 돌지 않는다. 게이트는 부수효과 없는 술어라
두 번 돌아도 판정이 같지만, **기록은 부수효과라 같은 축이 아니다** — 거부 한 건이 audit
행을 1 개 남기는지 2 개 남기는지를 따로 실측해 1 임을 확인했다.

`record_telemetry_and_audit`(Allow 기록)은 옮기지 않는다. 같은 이유에서다.

계약은 가드 `every_routing_entry_gates_before_it_answers` 가 소유한다. 판정은 **구문**이다
— caller 를 받아 라우팅하는 함수의 본문에서 게이트 호출 위치와 첫 `return` 위치를 비교하고,
게이트 결과가 곧바로 반환되는지까지 본다. 이름을 나열하지 않으므로 그런 함수가 새로
생기면 등재 없이 자동으로 대상이 된다.

## Consequences

- **얻은 것**: 조기 응답이라는 **부류 전체**가 검사를 건너뛸 수 없게 된다. 권한 · cap ·
  rate 셋이 한 자리에서 복구된다.
- **잃은 것**: 게이트가 두 번 도는 경로가 생긴다(통과 시). 술어라 부수효과가 없고 비용은
  해시셋 조회 수준이다.
- **남는 것**: 조기 응답 경로에는 Allow 텔레메트리가 기록되지 않는다.
- **운영 비용 / 유지 부담**: 가드가 구문 판정이라 유지할 목록이 없다. 면제는 하나이고
  그 근거를 테스트가 다시 검사한다.

### 측정 (2026-09-05 시점의 과거형 사실)

수정 전 audit `total 2 / deny 2 / allow 0`, 수정 후 `total 5 / deny 5 / allow 0` — 새로
막힌 3 건이 정확히 3 행 늘었다(메서드당 1 행, 중복 없음). 이 수는 메서드 구성이 바뀌면
낡는다. **다시 재는 방법**은 권한을 선언하지 않은 번들 plugin 으로 위 다섯 개를 부르고
`plugin audit-summary` 로 세는 것이다. 함정 셋:

- 격리된 `TASTY_HOME` 으로 인스턴스를 띄운다. 안 그러면 개발용 상태와 섞인다.
- GUI 는 `xvfb-run -a` 로 띄운다.
- CLI 는 부모 터미널의 `TASTY_SESSION_TOKEN` · `TASTY_SURFACE_ID` · `TASTY_AGENT_ID` ·
  `TASTY_PARENT_HOME` 를 상속하면 안 된다 — 상속하면 격리 인스턴스에 대해
  `session_token unknown/expired/revoked` 로 떨어지고, 그것을 결함으로 오독하게 된다.

## Alternatives Considered

- **A: `dispatch_list_global` 에만 caller 를 넘겨 게이트를 태운다** — 기각. 그 인스턴스
  하나만 막고 **cap · rate · audit 는 여전히 스킵**된다. 같은 결함을 세 축 남기는 셈이고,
  다음 조기 응답이 추가되면 아무것도 해주지 않는다. 보안 수정의 기준은 *그 인스턴스가
  아니라 그 부류를 불가능하게* 만드는 것이다.
- **B: plugin 진입부(`process_plugin_ipc_calls`)에 agent 와 대칭인 pre-gate 를 둔다** —
  기각이 아니라 **후속**. 비대칭은 실재하고 이 결정으로 없어지지 않는다. 다만 그 진입부의
  인터셉트 갈래들이 각자 게이트를 갖고 있어 중복 판단이 선행돼야 하고, 그건 이 결정과 다른
  결정이라 한 커밋에 섞으면 두 근거가 엉킨다. 판단 지점은 `src/app/dispatch/plugin_ipc.rs`
  의 갈래들과 `crates/tasty-host-plugin/src/manager/buffer.rs` 의 권한모델 TODO 다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- 게이트가 두 번 도는 경로의 비용이 실측으로 드러날 때 — 그때는 안쪽 게이트의 범위를
  다시 가른다.
- `plugin_ipc.rs` 의 인터셉트 갈래들이 갖는 자기 게이트가 바뀔 때.
- plugin 진입부의 비대칭이 닫힐 때(위 B) — 그 시점에 바깥 게이트의 일부가 중복이 된다.

## References

- [plugin-permissions](../dev-guide/plugin-permissions.md) — 권한 토큰별 메서드 목록
- [ADR-0141](0141-host-key-namespace-is-reserved-in-raw-memory-kv.md) — 같은 부류(메서드
  단위로는 일관되는데 조합에서 새는 것)의 다른 인스턴스
- `src/app/dispatch/intents.rs` — `gates_before_routing` 과 가드
- `src/adapters/ipc/handler.rs` — 안쪽 게이트 3종
- `src/app/ipc/caller_gate.rs` — agent 경로의 대칭 지점
