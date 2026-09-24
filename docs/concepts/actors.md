# 주체 (Actors)

Tasty는 한 인스턴스를 여러 주체가 동시에 사용하도록 설계한다. 주체마다 조작 방법과 권한이 다르다([기본 원칙](../identity.md)).

## 로컬 사용자 (Local user)

로컬 사용자는 이 머신에서 키보드·마우스·OS 입력으로 Tasty GUI를 직접 쓰는 사람이다. 포커스를 정하고 점유되지 않은 surface·workspace를 자유롭게 조작한다. 한 인스턴스에 보통 한 명이며, 다른 주체의 점유를 강제로 해제할 수 있다.

## AI Agent (에이전트)

에이전트는 IPC·CLI로 대상을 ID로 지정해 자기 작업을 수행한다. 여러 에이전트가 동시에 동작하더라도 사용자의 포커스·닫은 항목 히스토리·선택을 바꾸지 않는다. `surface.send`·`surface.read`처럼 점유 없이 요청할 수 있으며 필요하면 soft 또는 hard 점유를 사용한다. `terminal` 명령이 만든 child-terminal의 soft 점유가 한 예다([ADR-0021](../adr/0021-occupancy-and-attach-admission.md)).

## 원격 접속 사용자 (Remote user)

원격 사용자는 SSH를 통해 attach로 접속하는 사람이다. 로컬 GUI 입력 대신 attach 스트림을 사용하고 로컬 포커스를 바꿀 수 없다. 에이전트와 비슷한 제한을 받지만, 원격 사용자는 작업 전에 surface나 workspace를 반드시 hard 점유해야 하며 점유한 대상만 조작할 수 있다.

tasty 는 자체 원격 프로토콜이 없고 SSH 에 위임한다 — attach 동작은 [`../features/remote-attach/`](../features/remote-attach/index.md), 메커니즘은 [`../dev-guide/attach-behavior.md`](../dev-guide/attach-behavior.md).

## 점유 (Occupation) 모델

점유는 원격 사용자나 에이전트가 surface·workspace를 사용 중임을 계속 표시하는 관계다. 일회성 `surface.send`·`surface.read` 요청과 구분한다. soft는 green, hard는 peach 테두리로 표시한다. 선택 이유와 표시 규칙은 [ADR-0021](../adr/0021-occupancy-and-attach-admission.md)에 있다.

### 약한 점유 (soft)

- soft 점유는 사용 중임을 알리는 표시다. 대상이 닫히거나 상태가 바뀔 수 있음을 알리지만 입력을 제한하지 않는다. 로컬 사용자는 평소처럼 조작할 수 있다.
- 현 소비자: `terminal` 명령이 spawn 한 **child-terminal**(주체 = 그 child 를 spawn 한 parent surface) → [`../features/child-terminal/`](../features/child-terminal/index.md).

### 강한 점유 (hard)

- **배타 + readonly.** 점유한 주체만 조작하고, 그동안 **로컬 사용자·다른 주체는 그 대상에 대해 readonly** — 무슨 일이 일어나는지 *볼 수만* 있다.
- **원격 attach 가 이 계층의 사례** 다(원격 사용자는 hard 점유로만 동작) → [`../features/remote-attach/`](../features/remote-attach/index.md), 메커니즘 [`../dev-guide/attach-behavior.md`](../dev-guide/attach-behavior.md).

### 공통 규칙

- **해제 권한**: 어느 계층이든 점유는 **점유 주체 본인(self-release) 또는 로컬 사용자(force-detach)** 만 끊는다. 끊으면 대상은 다시 **일반 surface/workspace 로 복귀** 한다.
- **배타(1:1)**: 계층과 무관하게 **한 대상(surface/workspace)은 한 번에 한 주체만 점유** 한다. 점유는 *주체 → 대상* 1:N, *대상 → 점유자* 1:1. 동시 점유는 불가하고, 다른 주체가 잡으려면 기존 점유가 먼저 풀려야 한다.
- **다중성**: 원격 사용자는 여럿이 동시에 접속할 수 있고, 한 주체가 여러 대상을 동시에 점유할 수 있다.

## 정리

| | 로컬 사용자 | AI Agent | 원격 접속 사용자 |
|---|---|---|---|
| 부류 | 사람 | AI | 사람 |
| 동작 경로 | 직접 GUI 입력 | IPC / CLI | attach 연결 |
| 분류 성격 | 사용자 행동 | 에이전트 행동 | **에이전트에 가까움 + 점유** |
| 점유 | 불필요 | **선택** (soft/hard) | **필수** (강한 점유 안에서만) |
| 포커스 | 주인 | 안 건드림 | 로컬 포커스 비주인 |
| 타 점유 강제해제(force-detach) | **있음** | 없음 (자기 점유 self-release 만) | 없음 (자기 점유 self-release 만) |
| 동시 수 | 보통 1 | 0..N | 0..N |

API를 설계할 때는 로컬 직접 입력과 에이전트·원격 요청을 구분한다. [사용자와 에이전트 행동의 분리](../identity.md#21-사용자-행동--에이전트-행동-분리-soul)를 따른다.