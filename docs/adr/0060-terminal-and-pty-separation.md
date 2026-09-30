# ADR-0060: 터미널 내용과 OS PTY 연결을 별도 객체로 나누고 엔진이 한 곳에서 소유한다

- **Status**: Accepted — 구현 상태: 단계적 이행 중. 현재 `Terminal`이 VT 상태와 PTY 핸들을 함께 갖고, `TerminalStore`와 `PtyRegistry`가 각각 컬렉션을 가진다
- **Date**: 2026-09-30
- **Tags**: terminal, pty, lifecycle, ownership
- **Group**: terminal

## Context

현재 `crates/tasty-terminal/src/lib.rs`의 `Terminal`은 VT 상태와 PTY 핸들을 함께 가지며, 내부 `PtyBackend`가 OS master·child와
reader/parser·writer thread를 소유한다. `src/core/terminal_store.rs`의 `TerminalStore`는 `CoreState` 안에서 Terminal을 보관하고,
`src/core/pty_registry.rs`의 `PtyRegistry`는 surface 없는 PTY의 메타데이터·상한·유휴 정리·exit watcher를 따로 가진다.
원격 mirror의 Terminal에는 PTY가 없다.

[ADR-0013](0013-terminal-io-and-process-lifetime.md)은 이 구조에서 파서 스레드·잠금·PTY 수명·절전 복구·출력 스캐너 커서를 정했다.
화면 없는 PTY는 별도 registry로 제공하는 것이 당시 선택이었다.

[ADR-0054](0054-app-core-view-layers-and-state-ownership.md)와 [ADR-0055](0055-structural-domain-event-sourcing.md)에서는 재생 가능한 `CoreState`가
OS 자원을 가질 수 없고, 같은 자원을 여러 Store·Registry에 반복 등록하지 않아야 한다. PTY 없는 mirror, standalone PTY, surface 복원을 함께 지원하려면
내용과 OS 연결의 책임을 나눠야 한다.

## Decision

### 두 객체와 소유

| 객체 | 동작 | 상태 | private 자원 |
|---|---|---|---|
| Terminal | bytes ingest, VT 해석, grid resize 적용, 화면·scrollback snapshot과 검색 | `TerminalState`: grid, 프로그램 cursor·mode, scrollback, 내용 revision | 필요한 parser 실행 수단. OS child·master는 없음 |
| Pty | 프로세스 spawn, raw read/write, OS resize, exit 관측·종료·reap | `PtyState`: 연결 상태, resource generation, 종료 관측, standalone 소유·활동 | OS master·child, I/O 핸들, worker |

- 한 엔진의 Terminal·Pty 원본과 둘 사이의 연결은 `EngineSession`의 컬렉션 한 곳이 소유한다. `CoreState`는 논리 세션 사실만 가진다.
- 기존 `TerminalStore`·`PtyRegistry`·`PtyBackend`의 책임을 이 두 객체와 엔진 컬렉션으로 재배치한다. 그 위에 TerminalSessionManager·PtyManager 같은 관리자를 추가하지 않는다.
  standalone PTY의 메타데이터·상한은 `PtyState`와 엔진의 유일한 standalone 색인으로, exit watcher는 Pty의 child 소유·reap 경로로 합친다.
- 사용자 viewport·selection은 `ViewState`다. 프로그램 cursor·scrollback과 구분한다.
- 논리 세션 ID와 Pty의 resource generation을 구분한다. 이전 generation의 출력·exit가 respawn한 새 Pty를 바꾸지 않는다.
- Terminal이 만드는 DSR·DA·OSC 응답은 현재 generation의 local Pty writer 또는 mirror attach sink로 보낸다. 사용자 입력이나 로그 replay 입력으로 바꾸지 않는다.
- 높은 빈도의 raw I/O는 구조 이벤트 저널에 기록하지 않는다. 터미널 내용의 복원은 surface snapshot 계약을 따른다.
- 객체 분리와 crate 분리는 별개다. 두 객체는 `tasty-terminal` 안의 별도 모듈로 둔다([ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)).

연결 방식은 다음과 같다.

- 일반 terminal surface: 도메인 surface·세션과 Terminal을 연결하고 필요하면 Pty를 붙인다.
- standalone PTY: surface 없이 Pty와 필요한 Terminal을 유지한다. 기존 상한·TTL은 이 범위에만 적용한다. `pty.*` API는 surface를 만드는 `terminal.*`와 역할을 구분한다.
- adopt(`pty.attach_surface`): 같은 Terminal·Pty를 새 surface에 연결하고 프로세스를 다시 만들지 않는다.
- mirror: 원격 bytes로 `TerminalState`를 갱신하고 local Pty를 만들지 않는다.
- 지연 복원: 구조와 snapshot만 복원한 뒤 기존 활성화 시점에 Terminal·Pty를 초기화한다.

### 유지하는 동작 규칙

- 각 터미널의 PTY 읽기 worker가 VT 파싱과 grid 갱신을 맡고, 메인 이벤트 루프는 VT를 파싱하지 않는다. 같은 worker가 read→ingest를 수행해도 논리 소유와 OS 책임은 나뉜다.
- `TerminalState`는 터미널별 잠금으로 공유한다. 읽기 worker는 8KB 청크를 처리한 뒤 잠금을 놓고, 렌더링은 필요한 값을 한 번의 잠금에서 읽는다. 잠금 밖으로 grid 참조를 반환하지 않는다.
- snapshot과 출력 tap은 parser와 같은 일관성 경계에서 처리한다. 타입을 나누면서 각자 잠금을 잡는 두 호출로 만들지 않는다.
- resize는 grid 변경과 tap 통지가 먼저이고 OS resize는 예약 후 flush한다. tap을 OS resize 성공 확인으로 취급하지 않는다.
- PTY 자식은 PTY를 소유한 호스트와 수명을 함께한다. Windows는 `tasty-reaper`의 `KILL_ON_JOB_CLOSE` Job Object에 셸을 등록하고, 정상 닫기에서는 Pty가 자식을 명시적으로 종료한다.
  Unix의 비정상 종료는 PTY hangup과 SIGHUP에 의존한다. kill·wait 소유자는 한 곳이다. 닫기 완료·종료 신호 전달·실제 exit·reap 완료를 같은 것으로 보고하지 않는다.
- Windows 절전 복귀는 `WM_POWERBROADCAST`로 감지해 종료한 자식을 정리하고 살아 있는 PTY에 현재 크기로 resize를 보낸다. 응답이 없다는 이유로 자식을 강제 종료하거나 재생성하지 않는다.
- PTY EOF는 자식 종료와 구분한다. EOF 뒤에는 종료가 확인되거나 소유권이 이전되거나 Terminal이 사라질 때까지 10ms부터 두 배씩 최대 500ms 간격으로 host를 깨운다.
- 출력 스캐너는 에이전트 mark와 독립된 scan 커서를 쓰며, `take_since_scan_mark`는 읽기와 커서 전진을 한 번에 한다. `surface.read_since_scan_mark`는 커서를 소비하므로 재전달 분류가 Mutate다.

## Consequences

재생 가능한 도메인 모델이 OS 자원 없이 존재하고, PTY 없는 mirror와 standalone PTY가 같은 객체 모델을 쓴다.
Terminal·Pty 등록과 종료 책임이 한 곳에 있어 누락·중복 정리를 찾기 쉽다.

`tasty-terminal`의 PTY 생성·설정·`take_child`·resize·tap 소비자가 root 밖(CLI, host-plugin, model, selection, terminal-link)에도 있어
소비 API를 먼저 정리해야 한다. 이행 중 child가 외부 watcher에 넘어가는 단계에서도 kill·wait 소유자가 하나여야 한다.

## Alternatives Considered

- 현재 `Terminal`에 PTY를 계속 두고 `CoreState`에서 컬렉션만 옮기는 안: mirror·standalone·surface 경로의 수명 규칙이 계속 한 타입의 분기로 남는다.
- 기존 Store·Registry 위에 세션 관리자를 추가하는 안: 등록·수명·종료 책임이 세 곳으로 늘어난다.
- standalone PTY를 별도 registry로 계속 두는 안([ADR-0013](0013-terminal-io-and-process-lifetime.md)의 방식): 같은 자원의 두 번째 컬렉션이 되고 adopt 때 소유 이전이 두 컬렉션을 오간다.
- parser와 PTY reader를 다른 thread로 강제 분리하는 안: snapshot·tap 원자성이 어려워지고 이득이 측정되지 않았다.
- 별도 PTY crate를 바로 추출하는 안: 소비 API가 정리되지 않아 공개 범위만 넓어진다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 순수 터미널 재생·원격 mirror·renderer 테스트에서 PTY 의존을 빼야 하면 PTY crate 추출을 검토한다.
- Terminal과 Pty 사이에 독립 수명을 가진 세 번째 책임이 확인되면 그때 객체를 추가한다. 이름의 대칭만으로 추가하지 않는다.
- 엔진 컬렉션 밖에 Terminal·Pty의 두 번째 원본이 생기면 소유 배치를 다시 본다.

### 실행 결과로 확인

- parked 파싱의 배터리·CPU 비용이 문제로 관측되면 흐름 제어를 검토한다.
- 절전 복귀 후 입력이 멈추는 사례가 반복되면 복구 절차를 다시 본다.
- resize 순서나 snapshot·tap 경계를 바꾸면 원격 mirror에서 화면 어긋남이 없는지 확인한다.

## References

- 대체 대상: [ADR-0013](0013-terminal-io-and-process-lifetime.md)
- [ADR-0054](0054-app-core-view-layers-and-state-ownership.md) · [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md) · [ADR-0034](0034-output-cursor-contract.md)
- [터미널](../features/terminal/index.md), [헤드리스 PTY](../features/headless-pty/index.md)
- 현재 구현: `crates/tasty-terminal/src/lib.rs`, `src/core/terminal_store.rs`, `src/core/pty_registry.rs`.
