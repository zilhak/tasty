# ADR-0060: 터미널 내용과 OS PTY 연결을 별도 객체로 나누고 엔진이 한 곳에서 소유한다

- **Status**: Accepted — EngineSession의 TerminalStore가 Terminal과 선택적인 Pty를 한 항목에서 소유한다. 내용과 physical resource generation을 구분하고 activation·retirement는 effect/receipt 경계에 연결돼 있다. 원자 snapshot/ordered attach stream은 같은 parser lock에서 등록한다. 이 source 구조가 OS별 종료·Recovery·경합 실행 검증을 대신하지 않는다.
- **Date**: 2026-09-30
- **Tags**: terminal, pty, lifecycle, ownership
- **Group**: terminal

## Context

결정 당시 `crates/tasty-terminal/src/lib.rs`의 `Terminal`은 VT 상태와 PTY 핸들을 함께 가지며, 내부 `PtyBackend`가 OS master·child와
reader/parser·writer thread를 소유한다. `src/core/terminal_store.rs`의 `TerminalStore`는 `CoreState` 안에서 Terminal을 보관하고,
당시 `PtyRegistry`는 surface 없는 PTY의 메타데이터·상한·유휴 정리·exit watcher를 따로 가진다.
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
- EOF 직후 한 번의 `try_wait`가 아직 실행 중이라고 반환해도 나중에 종료를 확인할 수 있어야 한다. parser가 child wait를 직접 소유하지 않는다.
  PTY를 닫고 계속 실행하는 자식은 최대 500ms마다 host를 깨우며, Terminal을 버릴 때 parser 종료를 기다리지 않는다(최대 한 대기 간격 뒤 종료).
  targeted polling은 해당 터미널만 처리하지만 GUI에서 보이는 surface는 그 창을 다시 그릴 수 있다. targeted polling을 끄면 전체 터미널을 처리하고 모든 창을 다시 그릴 수 있다.
- 새 접근자는 상태 잠금을 거친다. mutex가 poison 상태이면 내부 값을 꺼내 복구한다. 백그라운드 터미널의 잠금은 사용자가 보는 터미널의 잠금과 독립이다.
- Windows 절전 복귀 처리에서 복구가 어려울 수 있는 surface는 사용자에게 알린다. 이 처리는 Windows에만 적용한다.
- Windows에서는 호스트 종료 시 `nohup`·백그라운드 작업도 종료되고, Unix에서는 SIGHUP을 무시한 프로세스가 남을 수 있다.
  Job 등록 실패는 경고를 남기고 기존 정리 방법을 사용한다. 플러그인과 터미널은 종료 구현을 공유하되 Job Object는 각 소유자가 따로 보관한다.
  클라이언트의 attach·detach는 서버가 소유한 셸의 수명을 바꾸지 않는다.
- standalone PTY(`pty.*`)는 surface를 만들지 않고 실제 종료 코드를 보관한다. 사용자가 탭을 보고 정리할 수 없으므로 개수 제한과 유휴 세션 회수를 두며,
  PtyState·Terminal·wake 등록을 같은 정리 경로에서 함께 정리한다.
- `pty.*`는 별도 Pty 권한을 만들지 않고 기존 `TerminalRead`·`TerminalWrite`·`TerminalSpawn`으로 설명한다. 화면 없는 작업을 `terminal.*` 옵션으로 넣지 않는다.
- 출력 스캐너는 에이전트 mark와 독립된 scan 커서를 쓰며, `take_since_scan_mark`는 읽기와 커서 전진을 한 번에 한다. 에이전트의 set/read/parse mark API는 그대로 유지한다.
  `surface.read_since_scan_mark`는 `TerminalRead` 권한으로 호출하고, 커서를 소비하므로 재전달 분류가 Mutate다.
  CLI 명령은 만들지 않지만 일반 로컬 IPC 호출을 막는 API는 아니다.
- scan 커서는 surface마다 하나다. 소비자가 둘이면 서로 읽을 데이터를 가져갈 수 있으므로 소비자는 하나라는 전제다. 첫 호출은 현재 보관한 출력을 모두,
  이후 호출은 새 출력만 반환한다. plugin은 새 출력만 자기 제한된 버퍼에 모으며, host와 plugin의 보관 상한은 같은 값이어야 한다.
  여러 소비자의 독립 읽기는 [ADR-0034](0034-output-cursor-contract.md)의 cursor 계약을 쓴다.

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
- Pty 전용 권한을 새로 만드는 안: 기존 `Terminal*` 세 권한으로 설명할 수 있고 plugin 권한 선언만 늘어난다.
- scan mark 대신 기존 `read_since_mark`에 선택 인자를 붙이는 안: 구 host가 인자를 무시하고 다른 범위를 돌려줄 수 있다. 새 이름은 미지원 오류로 구별된다.
- 절전 복귀를 winit `suspended`·`resumed`로 감지하거나 모든 OS에 적용하는 안: 데스크톱 절전을 감지하지 못하고 문제가 확인되지 않은 Unix PTY까지 다시 그린다.
  멈춘 프로세스와 정상 대기를 구분할 수 없으므로 자동 종료·재실행도 하지 않는다.
- Drop만으로 자식을 정리하는 안: 크래시·강제 종료에서 Windows 자식이 남는다. 다음 부팅에서 고아를 찾아 종료하는 안은 다른 호스트의 자식과 혼동할 수 있다.
- DAG runner의 subprocess로 standalone PTY를 대신하는 안: stdout·stderr 캡처 구조라 PTY 화면 조회와 입력 전달을 대신하지 못한다. 종료 코드 watcher 방식만 공유한다.
- EOF 뒤 모든 host가 모든 터미널을 주기 검사하는 안: 원인과 무관한 터미널까지 반복 처리한다. SIGCHLD는 Windows에 같은 기능이 없어 공통 해법으로 쓰지 않는다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 순수 터미널 재생·원격 mirror·renderer 테스트에서 PTY 의존을 빼야 하면 PTY crate 추출을 검토한다.
- Terminal과 Pty 사이에 독립 수명을 가진 세 번째 책임이 확인되면 그때 객체를 추가한다. 이름의 대칭만으로 추가하지 않는다.
- 엔진 컬렉션 밖에 Terminal·Pty의 두 번째 원본이 생기면 소유 배치를 다시 본다.
- scan API의 두 번째 소비자가 생기면 소비자별 커서를 도입한다. host와 plugin의 출력 상한이 달라지면 스캐너가 새 데이터를 받는지 다시 점검한다.
- standalone PTY의 상시 표시나 권한 분리 요구가 생기면 standalone 색인과 권한 경계를 다시 본다.
- Unix에서 자식 수명을 호스트에 연결하는 API나 ConPTY 종료 보장이 추가되면 OS별 종료 차이를 줄일 수 있는지 검토한다.

### 실행 결과로 확인

- parked 파싱의 배터리·CPU 비용이 문제로 관측되면 흐름 제어를 검토한다.
- 절전 복귀 후 입력이 멈추는 사례가 반복되거나 Unix에서도 같은 문제가 재현되면 복구 절차를 다시 본다.
- standalone PTY의 개수·TTL·회수 지연이 장수명 작업에 맞지 않는 사례가 나오면 기본값과 설정 범위를 재검토한다.
- 청크 잠금 경합이 프로파일에서 확인되면 읽기용 snapshot이나 이중 버퍼를 검토한다.
- resize 순서나 snapshot·tap 경계를 바꾸면 원격 mirror에서 화면 어긋남이 없는지 확인한다.

## References

- 대체 대상: [ADR-0013](0013-terminal-io-and-process-lifetime.md)
- [ADR-0054](0054-app-core-view-layers-and-state-ownership.md) · [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md) · [ADR-0034](0034-output-cursor-contract.md)
- [터미널](../features/terminal/index.md), [헤드리스 PTY](../features/headless-pty/index.md)
- 현재 구현: `crates/tasty-terminal/src/lib.rs`, `crates/tasty-terminal/src/pty.rs`, `src/runtime/terminal_store.rs`, `src/runtime/terminal_store/standalone.rs`.
