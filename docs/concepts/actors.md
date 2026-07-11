# 주체 (Actors)

tasty 는 같은 인스턴스를 여러 주체가 **동시에** 사용하는 것을 전제로 설계된다 (→ [identity.md](../identity.md) 동시성). 세 주체는 *무엇을 통해 동작하는가* 와 *어떤 계약을 따르는가* 가 각각 다르다.

## 로컬 사용자 (Local user)

이 머신에서 tasty GUI 를 직접 쓰는 사람. 입력 표면 = 키보드 단축키·마우스·OS 네이티브 입력. **포커스의 주인**. 일반(비점유) surface/workspace 를 자유롭게 다룬다. 한 인스턴스에 보통 1명. **점유를 끊을 수 있는 유일한 주체** (아래 점유 모델).

## AI Agent (에이전트)

자기 작업을 수행하기 위해 tasty 를 조작하는 AI. 입력 표면 = IPC 메서드 / CLI 서브커맨드, 대상은 ID 로 지정. 여럿이 동시에 동작하며 **격리 계약** 을 따른다 — 자기 행동의 부수효과가 사용자 상태(포커스/닫은 항목 히스토리/선택)에 닿지 않는다. **기본은 점유 없이** ID 로 임의 대상을 조작하지만(fire-and-forget `surface.send`/`surface.read`), 필요하면 **점유(soft/hard)를 걸 수 있다** — 예: `terminal` 명령이 spawn 한 child-terminal 을 soft 점유로 표시한다(아래 점유 모델, [ADR-0040](../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md)).

## 원격 접속 사용자 (Remote user)

SSH 너머에서 attach 로 접속하는 사람. **행동 분류는 로컬 사용자보다 AI Agent 에 가깝다** — 직접 GUI 입력이 아니라 *연결(attach 스트림)* 을 통해 동작하고, 로컬 포커스의 주인이 아니다. AI Agent 와의 결정적 차이는 **점유가 필수인가** 다:

- 원격 사용자는 무언가를 하기 전에 **반드시 surface 또는 workspace 를 강한 점유(hard, 배타 claim) 선언** 해야 하고, **점유한 대상 안에서만** 동작할 수 있다. 원격 사용자가 건드릴 수 있는 것은 *점유된 터미널/workspace* 뿐이다.
- AI Agent 는 점유 없이 ID 로 임의 대상을 조작할 수 있고 점유는 **선택**이지만, 원격 사용자는 **점유라는 관문을 반드시 통과** 한다.

tasty 는 자체 원격 프로토콜이 없고 SSH 에 위임한다 — attach 동작은 [`../features/remote-attach/`](../features/remote-attach/index.md), 메커니즘은 [`../dev-guide/attach-behavior.md`](../dev-guide/attach-behavior.md).

## 점유 (Occupation) 모델

점유는 **주체(원격 사용자 | AI Agent)가 surface/workspace 에 대해 선언하는 지속적·가시적 관계** 다. `surface.send`/`surface.read` 같은 fire-and-forget 조작과 달리, "이 대상은 지금 어떤 주체가 조종 중" 이라는 사실을 로컬 사용자에게 명시한다. 두 계층이 있고 **시각적으로 구분** 된다(터미널 테두리 색: soft=green, hard=peach). 결정 근거·시각 규약은 [ADR-0040](../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md).

### 약한 점유 (soft)

- **표시만 하는 advisory 마커.** "이 대상은 어떤 주체에게 조종당하는 중이며 언제든 닫히거나 상태가 바뀔 수 있다" 를 고지한다. **write 제한 없음** — 로컬 사용자는 평소처럼 자유롭게 입력·조작한다. 협조 신호이지 강제가 아니다.
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

"사용자 행동(로컬 직접 입력) ↔ 에이전트 행동(연결 기반)" 분리가 tasty 의 soul 이며, 모든 API 설계가 그 위에 얹힌다 (→ [identity.md](../identity.md) §2.1).
