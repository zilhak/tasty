# CLI ↔ IPC 표면 — 무엇이 CLI 로 닿고, 무엇이 왜 안 닿는가

[`docs/identity.md`](../identity.md) 원칙 2 는 에이전트 기능이 **IPC 와 CLI 양면**으로
동작해야 한다고 못 박는다. 이 문서는 그 대조를 **어떻게 판정하고 어떻게 세는지**를 적는다.
결정의 근거·대안·재검토 조건은
[ADR-0160](../adr/0160-every-ipc-method-is-cli-reachable-or-carries-a-reason.md).

**어느 메서드가 CLI 없이 남아 있고 그 사유가 무엇인지의 정본은
[api-conventions](api-conventions.md) 의 두 표**다(release 절반 · debug 절반).
`tests/cli_method_table_parity.rs` 가 그 표와 실제 집합을 **양방향으로** 대조하므로,
진입점이 생기면 행을 지워야 하고 새 메서드를 CLI 없이 얹으면 행을 넣어야 한다. 목록을
여기 옮겨 적지 않는다 — 두 벌이 되는 순간 한쪽만 고쳐진다.

## 판별식

> **호출자가 누구인지가 응답의 일부인가.**

응답이 호출자의 신원(자기 배너·자기 팝업·자기 plugin 설정)이나 호출자에게 push 되는
이벤트 수신처에 매여 있으면 셸은 호출자가 될 수 없다 — 셸에는 plugin 신원도 이벤트
수신처도 없다. 매여 있지 않으면(전역 스냅샷 조회든 id 로 대상을 지정하는 쓰기든)
진입점이 있어야 한다.

**이 판별식은 "전형적 호출자가 plugin 인가" 와 다르다.** 뒤엣것은 *관행*이고 앞엣것은
*불가능성*이다. 관행으로 가르면 `surface_id` 를 인자로 받아 셸도 부를 수 있는 메서드가
"plugin 이 자기 surface 를 위해 부른다" 는 이유로 진입점 없이 남는다 — 실제로 그렇게
남아 있던 것이 여섯이었다(ADR-0160 의 "판별식이 이전 기준을 대체한다").

## 어떻게 세는가

**실행으로 센다.** 이름이 비슷한 잎을 찾는 방식은 두 방향으로 틀린다.

- 플래그 뒤에 숨은 진입점을 못 본다. `message.clear` 는 `tasty read queue --clear` 가,
  `surface.send_wait_idle` 은 `tasty send text --wait-idle` 이 보낸다 — 서브커맨드
  이름에는 그 메서드가 없다.
- **와이어 침묵을 진입점 부재로 오해한다.** `tasty tool remote-profile add-ssh` 는 rc=0
  인데 IPC 를 한 번도 안 탄다 — `crates/tasty-cli/src/local/` 이 그 자리에서 실행한다.
  그런 명령은 진입점이 **있는** 것이다.

세는 절차는 살아 있는 인스턴스 앞에 프록시를 세워 각 CLI 잎이 실제로 실은 메서드를
관측하는 것이다. 인자를 못 맞춰 실행이 안 된 잎은 **미측정**이지 부재가 아니다 — 그
편향은 한쪽으로만 작용하므로(부재 집합은 줄어들 수만 있다) 상한으로만 쓴다. 실제로
재확인하니 "잎은 있는데 인자를 못 맞춘 것" 22 건이 전부 진입점 있음으로 바뀌었다.

가드가 소스에서 판정할 때 쓰는 "CLI 로 닿는다" 의 정의는 세 갈래다
(`cli_reachable_methods`): CLI 의 요청 조립 자리에 있는 값 위치 리터럴, 크레이트 전체의
`method: "…"` 필드, 그리고 **번들 plugin 매니페스트의 `ipc_method`**. 마지막 것이 없으면
`tasty image open` 처럼 plugin 이 기여하는 명령이 전부 "진입점 없음" 으로 잘못 잡힌다.

## 선행 작업이 필요해 미룬 것

- **`markdown.navigate` 의 CLI 진입점** — namespace 를 번들 plugin 이 점유해 외부 호출이
  plugin 으로 forward 된다([ADR-0153](../adr/0153-a-bundled-namespace-hands-host-methods-back.md)).
  host 잎을 만들면 plugin 설치 여부에 따라 흔들리므로, 진입점은 plugin 의 매니페스트
  `ipc_method` 기여로 가야 한다 — plugin 크레이트 수정 + 매니페스트/Cargo 버전 bump.

## 관련 문서

- [api-conventions](api-conventions.md) — **사유 표의 정본**(release · debug 두 표)
- [ADR-0160](../adr/0160-every-ipc-method-is-cli-reachable-or-carries-a-reason.md) — 이 규칙의 결정
- [headless-ipc-surface](headless-ipc-surface.md) — 같은 표를 조합(gui/headless) 축으로 가른 대조
- [debug-ipc](debug-ipc.md) — debug 격리 정책과 CLI 의 debug 트리
