# ADR-0401: 원격 연결 상태 사건은 사용자 행동 없이도 toast 를 띄울 수 있다 — toast 트리거 정책의 허용 부류

- **Status**: Accepted
- **Date**: 2026-09-21
- **Tags**: toast, attach, mirror, ui, identity-principle-1, user-agent-separation

## Context

[toast.md](../design/systems/toast.md) "트리거 정책" 은 첫 문장에서 "Toast 는 사용자 행동(키보드
단축키 / 마우스)에서만 발사된다" 고 적는다. 근거는 [identity](../identity.md) 원칙 1 이다 — 에이전트
행동(IPC/CLI)의 부수효과가 사용자 시각 상태에 닿지 않아야 한다.

그런데 attach mirror 는 결정 시점에 이미 사용자 행동이 아닌 사건에서 toast 를 띄우고 있었다.

- `attach.toast.mirror_reconnecting` — 원격 연결이 끊겨 재연결을 시도한다.
- `attach.toast.mirror_reconnected` — 재연결에 성공했다.
- `attach.toast.mirror_disconnected` — 연결이 끊겨 mirror 를 닫았다.
- `attach.toast.mirror_structural_forward_failed` — mirror 에서 한 구조 변경을 원격이 적용하지 못했다
  (조작은 사용자가 했지만 실패는 원격 왕복 뒤에 온다).

[ADR-0400](0400-attach-loss-is-resynced-per-connection-with-the-strongest-contract-it-carries.md) 은
여기에 `attach.toast.mirror_desynced`(원격 화면 일부를 놓쳐 다시 받는다)를 더했다. 정책 문언과
현실이 어긋난 채였고, 새 toast 가 정책 위반인지 선례를 따른 것인지 문서만 보고는 가를 수 없었다.

이 사건들의 원인은 에이전트의 IPC 가 아니다. 네트워크 단절, 원격의 처리 결과, client 소비 속도가
원인이다. 원칙 1 의 좌변("에이전트 행동의 부수효과")에 들지 않는다. mirror 세션을 에이전트가
IPC(`remote attach --into-gui`)로 열었더라도, 그 세션 위의 연결 사건은 IPC 호출의 직접 결과가 아니다.
사용자가 보고 있는 작업 공간의 상태가 바뀐 것이다.

## Decision

toast 트리거 정책에 **허용 부류 하나**를 명시한다. **원격 연결 상태 사건**(끊김 · 재연결 · 손실 ·
구조 전달 실패)은 사용자 행동 없이도 toast 를 띄울 수 있다.

- 이 부류는 **사용자가 보고 있는 mirror 작업 공간이 믿을 만한가**를 알린다. mirror 는 원격의
  사본이라 연결 상태가 곧 화면의 신뢰도다. 알리지 않으면 사용자는 낡은 화면을 최신으로 읽는다.
- 창 없는(parked) engine 에서는 띄우지 않는다. 표면이 없고 toast 수명은 wall-clock 이다
  (`MirrorHost::toast` 의 기존 게이트).
- **에이전트 IPC 호출이 직접 일으킨 결과**(IPC 쓰기의 성공/실패, IPC 로 요청한 창 생성 실패 등)는 이
  부류가 아니다. 종전대로 toast 하지 않는다([ADR-0117](0117-window-and-modal-creation-failure-policy.md)).
- 코드는 바꾸지 않는다. 문서를 이미 있는 동작에 맞춘다.

## Consequences

- **얻은 것**: 정책 문언이 현실과 맞는다. 다음에 연결 상태 toast 를 더하는 사람은 허용 여부를 이
  부류로 판정할 수 있다. `mirror_desynced` 가 선례를 따른 것임이 문서에 남는다.
- **잃은 것**: "사용자 행동에서만" 이라는 단일 규칙이 예외 하나를 갖는다. 예외 경계(연결 상태 사건
  vs IPC 결과)는 사람이 판정해야 한다.
- **운영 비용 / 유지 부담**: 한 번의 손실에 toast 가 둘 난다(손실 경고 + 재연결 성공). 느린 링크에서
  재attach 가 되풀이되면 쌍으로 반복된다. 같은 스코프에서 500ms 안에 같은 메시지가 오면 합쳐지는
  기존 규칙이 일부를 흡수하지만, 두 메시지는 서로 다르다.

## Alternatives Considered

- **(b) 손실 toast 를 없애고 재연결 toast 하나로 낡음 표시를 겸한다** — 안 골랐다. 재attach 가
  실패하거나 창 없는 동안 미뤄지면 재연결 toast 가 오지 않는다. 그 사이 사용자는 공백 난 화면을 최신으로
  읽는다. ADR-0400 이 "상태 = 낡음 표시" 를 계약으로 정했으므로 표시를 없애려면 그 계약부터 바꿔야 한다.
- **(c) 정책 문언을 그대로 두고 현 상태를 유지한다** — 안 골랐다. 문서와 동작이 어긋난 채로 남는다.
  다음 리뷰가 같은 질문을 다시 연다. 새 toast 를 막을 근거도 허용할 근거도 문서에 없다.
- **각 toast 를 개별 예외로 열거한다** — 안 골랐다. 목록은 새 toast 가 생길 때마다 자라야 하고, 부류를
  말하지 않으면 새 후보를 판정할 기준이 없다. 부류를 정의하고 현재 구성원을 예시로 든다.

## Reconsideration Triggers

**원리적으로 안 붙는 것**

- 한 번의 손실에 toast 두 개가 반복되는 것이 관측되면 (b) 를 다시 연다. 재는 법: 느린 링크(또는
  소비자 SIGSTOP)로 손실을 되풀이해 유도하고, 창 하나에서 1 분 동안 뜬 `mirror_desynced` ·
  `mirror_reconnected` 쌍의 수를 센다. 격리 인스턴스 둘로 재는 절차는 ADR-0400 의 트리거 절에 있다.
- 사용자가 연결 상태 toast 가 방해된다고 보고하면(배너나 사이드바 표시로 옮길지 재검토).

## References

- [design/systems/toast.md](../design/systems/toast.md) "트리거 정책" — 이 결정이 고친 문언
- [ADR-0400](0400-attach-loss-is-resynced-per-connection-with-the-strongest-contract-it-carries.md) — `mirror_desynced` 를 더한 결정
- [ADR-0117](0117-window-and-modal-creation-failure-policy.md) — 에이전트 요청 결과는 toast 하지 않는다
- [identity.md](../identity.md) 원칙 1
- 코드 근거(결정 시점의 현재 위치): `src/app/attach_client.rs` 의 `MirrorHost::toast` 와 네
  `attach.toast.mirror_*` 발사 자리
