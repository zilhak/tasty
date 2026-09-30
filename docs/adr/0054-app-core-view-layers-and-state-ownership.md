# ADR-0054: App·Core·View가 각자의 상태를 소유하고 엔진 수명을 창과 분리한다

- **Status**: Accepted — 구현 상태: 단계적 이행 중. 현재 코드는 창별 `AppState`와 `CoreState`의 직접 소유 구조이며 아래 배치로 옮기는 중이다
- **Date**: 2026-09-30
- **Tags**: architecture, state, ownership, lifecycle, domain
- **Group**: foundation

## Context

현재 `src/state.rs`의 `AppState`는 이름과 달리 창별 상태다. 실행 큐, 도메인 정리, 선택·스크롤 같은 사용자 상태와 GUI 자원을 함께 가진다.
`src/view/main.rs`의 `MainView`가 `CoreState`를 직접 소유하고, 창이 없는 엔진은 `src/app.rs`의 `App::parked_states`에
`(AppState, CoreState)` 짝으로 옮겨 보관한다. `src/core/state.rs`의 `CoreState`에는 활성 workspace·tab 인덱스와
`TerminalStore`, 훅·작업 대기 자원이 함께 있다.

[ADR-0002](0002-domain-execution-and-ports.md)는 이 구조를 전제로 결정했다. 창 상태를 별도 구조체로 복제하지 않고,
도메인이 필요한 창 연산을 `CascadeWindow`·`IpcWindow` 같은 port로 받으며, IPC와 forward가
`core::structural_exec`를 함께 호출하게 했다. 당시 전제는 "창 상태 없이 도메인 작업을 끝내기 어렵다"와
"상태를 나누는 비용이 크다"였다.

이 전제는 새 요구에서 성립하지 않는다. 엔진은 창보다 오래 살 수 있고(parked, headless),
에이전트 요청은 사용자 선택을 바꾸지 않아야 하며, 구조 변경은 기록한 이벤트로 재생할 수 있어야 한다
([ADR-0055](0055-structural-domain-event-sourcing.md)). 사용자 선택과 OS 자원이 도메인 모델에 섞여 있으면
재생이 OS 자원을 요구하거나 과거 사용자 선택을 다시 실행하게 된다.

## Decision

세 계층의 역할을 다음처럼 정하고, 각 역할의 상태를 해당 State가 소유한다.
세 계층은 객체 종류의 상한이 아니다. 독립된 수명·원본·실행 계약이 있는 책임은 별도 객체로 둔다.

| 계층 | 객체 | 상태 | 소유하지 않는 것 |
|---|---|---|---|
| App | 실행 시작·종료, 외부 입력 수신, 대상 해소, Core·View·서비스 조립 | `AppState`: 실행 단계, 전역 modal, 기본 IPC 라우팅 문맥, View↔엔진 연결, 앱 수준 대기 요청. 프로세스당 하나 | 도메인 원본의 복제, VT parser, 저널 구현 |
| Core | 명령 판단과 확정 이벤트 적용, 도메인 불변식 | `CoreState`: 엔진별 구조·kind descriptor·논리 세션 사실과 stream revision. 외부 자원 없이 재생 가능 | wire 응답, 디스크 commit, PTY 실행, 사용자 포커스, OS 창 |
| View | 로컬 OS 창 하나에 표시하고 사용자 입력을 해석 | `ViewState`: 선택·viewport·popup·IME·draft·표시 revision. 공통 상태와 종류별 상태를 합성 | 도메인 원본, 외부 연결 수명 |

엔진은 프로세스나 OS 창과 다른 도메인 실행 단위다. `EngineSession`이 한 엔진의 `CoreState`,
현재 점유·busy 같은 연결 종속 상태(`LiveDomainState`), kind별 surface 실행 객체와 Terminal/Pty 컬렉션을 소유한다.
엔진 목록의 원본은 App이 가진 `EngineSession` 집합 하나다. View 목록의 원본은 View registry다.
`AppState`의 View↔엔진 연결은 관계만 보관하고 두 목록을 복제하지 않는다. `EngineSession`에 반대 방향의
쓰기 가능한 연결을 따로 두지 않는다.

- `Core` 인스턴스와 `CoreState`는 1:1이 아니다. 같은 Core가 여러 엔진 상태를 판단하며 `CoreState`를 전역 하나로 합치지 않는다.
- parked는 살아 있는 엔진에 연결된 OS View가 없는 상태다. 엔진을 다른 컬렉션으로 옮기지 않는다.
  다시 보여 줄 사용자 상태는 View 복원 자료로 따로 두며 GPU·창을 붙잡지 않는다.
- 도메인 작업은 로컬 View 없이 완료된다. 필수 정리를 View가 대신하지 않는다. 창 연산 port로 도메인에 창을 빌려주는 방식은 쓰지 않는다.
- Core는 App·View·IPC 핸들러·wire 응답·OS 창을 직접 참조하지 않는다. View는 읽기 모델과 자기 상태로 그리고,
  명시 대상과 요청 출처를 담은 요청을 내보낸다.
- IPC schema·transport·호출자 인증은 진입 계층이다. 진입점 권한 검사와 Core의 점유·구조 불변식 검사는 서로를 대체하지 않는다.
- Settings·Plugins·Preset·Quit 창에는 가짜 `CoreState`를 만들지 않는다. 필요한 서비스와 자기 상태를 사용한다.
- OS 창·GPU·socket·worker·PTY 같은 실행 자원은 모든 State에 강제로 넣지 않고 해당 객체의 private 자원으로 둔다.
  State라는 이름은 직렬화나 영속을 뜻하지 않는다.

제품 계약은 유지한다. GUI의 main 창마다 독립 작업 공간을 가지며 같은 엔진을 여러 로컬 View에 동시 표시하는 기능은 만들지 않는다.
창이 없어진 것과 엔진이 종료된 것을 구분한다. 기존 IPC·CLI의 대상 생략·귀속 호환 규칙은 가짜 View를 만들지 않고 명시적으로 유지한다.

명령의 기록·실행·복원 객체는 [ADR-0055](0055-structural-domain-event-sourcing.md), crate 배치는
[ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md), 사용자 선택과 ID 대상은
[ADR-0059](0059-id-targets-and-view-owned-selection.md)에서 정한다.

## Consequences

도메인 작업의 완료가 창의 존재에 의존하지 않는다. parked·headless·원격 요청이 같은 실행 경로를 쓸 수 있고,
재생 가능한 모델과 실행 자원을 나눠 검사할 수 있다. 사용자 선택이 View에만 있으므로 에이전트 요청이 선택을
바꾸는 경로를 타입 경계에서 찾기 쉽다.

이행 비용이 크다. 현재 `AppState`·`CoreState` 필드를 원본별로 재배치하고, 같은 커밋에서 모든 소비자를 옮겨야 한다.
타입 이름만 바꾸거나 한 단계에서 타입만 지우고 호출부를 남기는 중간 상태는 허용하지 않는다.
이행이 끝날 때까지 현재 구조를 설명하는 문서와 이 결정이 다르며, 문서는 구현 단계마다 실제 구조로 갱신한다.

같은 crate 안에서는 컴파일러가 모듈 방향을 막지 못한다. 기존 `domain_does_not_reach_up` 같은 소스 검사는
새 경계를 확인하도록 구현 단계에서 전환해야 한다.

## Alternatives Considered

- 창별 `AppState`를 그대로 두고 필드만 정리하는 안: 엔진 수명이 창에 묶인 채 남고 parked 전환이 계속 상태 이동을 요구한다.
- 전역 `AppState`를 추가하고 기존 창 상태도 유지하는 안: 같은 이름의 두 상태가 생기고 원본이 둘이 된다.
- 모든 상태를 App·Core·View 세 State에 넣는 안: 저장·실행·소비·복구 책임이 세 객체에 몰리고 OS 자원이 재생 모델에 섞인다.
- 창 연산 port를 계속 넓히는 안([ADR-0002](0002-domain-execution-and-ports.md)의 방식): 창 없이 완료해야 하는 작업에서도 창을 빌려야 하고,
  port가 결국 창 상태 전체를 노출한다.
- 같은 엔진을 여러 로컬 View가 표시하는 구조로 확장하는 안: 요구가 없고 선택·포커스 규칙을 새로 설계해야 한다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 한 엔진을 여러 로컬 View에 동시 표시하는 기능이 제품 요구가 되면 View↔엔진 연결과 `ViewState` 범위를 다시 정한다.
- 도메인 작업이 View나 창 연산 없이 완료되지 않는 새 경로가 생기면 그 경로를 이 결정의 예외로 두지 말고 원인을 먼저 찾는다.
- `EngineSession` 밖에 같은 엔진 목록이나 Terminal/Pty 컬렉션의 두 번째 원본이 생기면 소유 배치를 재검토한다.

### 실행 결과로 확인

- 창 0개 parked 상태, headless 요청, 두 창의 독립 선택, 원격 attach 사례에서 값의 원본 소유자와 실행 주체를 하나씩 지목할 수 없으면 경계를 다시 본다.

## References

- 대체 대상: [ADR-0002](0002-domain-execution-and-ports.md) — 계층·상태 소유는 이 문서, crate 배치는 [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)
- [ADR-0001](0001-crate-dependency-boundaries.md) — crate 의존 방향 원칙
- [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0058](0058-headless-without-local-views.md) · [ADR-0059](0059-id-targets-and-view-owned-selection.md)
- 현재 구조 설명: [아키텍처](../architecture/index.md), [멀티 윈도우](../architecture/multi-window.md), [AppState 소유권](../dev-guide/app-state-ownership.md)
- 현재 구현: `src/app.rs`, `src/state.rs`, `src/core/mod.rs`, `src/core/state.rs`, `src/view/main.rs`, `src/view/base.rs`.
