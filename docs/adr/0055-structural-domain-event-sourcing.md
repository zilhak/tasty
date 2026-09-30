# ADR-0055: 구조 도메인은 확정 이벤트를 원본으로 삼고 기록·실행·복원을 별도 객체가 맡는다

- **Status**: Accepted — 구현 상태: 단계적 이행 중. 현재 구조 변경은 메모리 상태를 직접 바꾸고 레이아웃 snapshot이 복원 원본이다. 아래 객체는 아직 제품 경로에 연결되지 않았다
- **Date**: 2026-09-30
- **Tags**: architecture, event-sourcing, persistence, recovery, commands
- **Group**: foundation

## Context

현재 구조 변경은 `src/core/intent.rs`와 `src/app/dispatch/intents.rs`에서 상태와 PTY를 직접 바꾸고,
결과와 후속 처리 요청을 한 `CoreEvent` 값에 섞어 반환한다. GUI는 Intent 큐를 거치지만 IPC 동기 경로·plugin·원격 forward·시스템 복구는
각자 다른 지점에서 상태를 바꾼다. 재시작 복원의 원본은 `src/core/layout_persistence`의 레이아웃 snapshot이며,
`crates/tasty-host-plugin/src/event_bus.rs`의 EventBus와 이벤트 피드는 최근 통지를 보관하는 메모리 링이다.

이 구조에서는 어떤 변경이 언제 확정됐는지, 응답 유실 뒤 같은 요청을 다시 실행해도 되는지,
생성 도중 종료된 작업을 어떻게 정리할지 판단할 원본이 없다. 사용자는 구조 도메인을 이벤트 소싱 구조로 바꾸기로 결정했다.
[ADR-0054](0054-app-core-view-layers-and-state-ownership.md)의 계층 구분만으로는 기록·실행·소비·복구 책임이 정해지지 않는다.

## Decision

### 적용 범위

첫 적용 범위는 engine/workspace/category/pane/tab/surface 구조와 이름·소속·분할 비율, 논리 터미널 세션의 수명 사실,
구조 작업의 진행·보상·정리와 사용자 undo가 참조하는 구조 기록이다. 이 범위에서는 영속 이벤트가 유일한 원본이고
domain snapshot과 조회 모델은 파생값이다. 범위 안의 모든 writer가 새 경계로 옮겨지기 전에는 해당 범위의 저널을 원본으로 활성화하지 않으며,
같은 대상에 옛 writer와 새 writer를 섞지 않는다.

다음은 범위 밖이며 각자의 원본을 유지한다. 이벤트 저널과 원자적으로 함께 바뀐다고 주장하지 않는다.

- 터미널 grid·scrollback(Terminal), 실제 PTY·프로세스(Pty), busy·단기 attention·live 점유(`LiveDomainState`).
- 외부 연결·ID mapping·구독 진행(Remote), 선택·scroll·popup(View).
- 설정·memory·secret·approval·agent task·hook·plugin 내부 데이터 같은 기존 서비스 데이터.

범위 확장은 별도의 완결된 전환 단위로 한다. 기존 서비스 데이터를 이벤트 payload에 통째로 복제하지 않는다.

### 메시지 구분

Intent(UI 의도), Command(대상·입력이 고정된 요청), DomainEvent(확정 사실), CommandResult(요청 결과),
Effect(확정된 의무에 따른 외부 실행), Observation(자원에서 관측한 입력), Notification/projection(소비자별 표현)을 구분한다.
이미 일어난 사실과 앞으로 할 일을 한 값으로 영속하지 않는다. plugin EventBus·이벤트 피드·debug tracing은 이벤트 저장소를 대체하지 않는다.

### 객체와 책임

| 객체 | 책임 | 갖지 않는 책임 |
|---|---|---|
| CommandExecutor | 호출자·대상 문맥, 중복 명령 조회, stream 순서, decide→commit→apply→응답 | UI 그리기, 직접 PTY 구현 |
| Core | 순수 `decide`와 `evolve`. 입력은 명령과 고정된 문맥 값 | 디스크 commit, wire 응답, 외부 실행 |
| EventStore | event batch·command identity·effect·consumer checkpoint·snapshot의 저장 계약 | 도메인 판단, 원격 wire 정의 |
| EffectRunner | 확정된 effect를 adapter로 실행하고 결과·재시도·불명확 상태를 기록 | replay 중 effect 재생성, 자원 소유 |
| ProjectionCoordinator | 소비자별 확정 이벤트 전달, checkpoint, 조회·원격 projection | View를 작업 분담 소비자로 취급 |
| RecoveryCoordinator | writer fencing, snapshot+tail 재구성, 미완료 operation 대조 | surface 내부 복원 정책 결정 |
| SurfaceRestorer | kind별 저장 상태를 기존 복원 구현으로 전달, 즉시·지연 초기화 연결 | 과거 명령을 실행 요청으로 재전달 |
| IdentityAllocator | 재사용하지 않는 typed ID 범위의 영속 예약 | 포커스·OS PID를 도메인 ID로 사용 |

객체마다 thread나 crate를 요구하지 않는다. 같은 자원을 여러 Store·Registry·Manager에 반복 등록하지 않으며,
동작 없는 facade를 이름의 대칭 때문에 추가하지 않는다.

### 확정 경계

- GUI·IPC·plugin·원격·시스템 요청이 모두 같은 CommandExecutor로 합류한다. Intent 큐 사용은 필수가 아니다.
  범위 안의 모델은 이 경계를 거치지 않고 바꾸지 않는다. 결과는 도메인 값이며 wire 오류·응답 조립은 진입점이 한다.
  같은 입력에는 진입점과 무관하게 같은 검증·실패 사유를 내고 기존 실패 문구를 유지한다. 공용 정수·범위 검사는 도메인에 두되 IPC 전용 별칭 해석은 진입 계층에 둔다.
  닫힌 surface의 회수와 닫기 결과 변환은 공용 경로 하나가 맡는다. 다른 소유자의 자원 정리(plugin mesh frame 등)는 실행 결과로 알리고 그 소유자가 수행한다.
- 같은 command identity 조회는 대상 존재 검사와 포커스 해소보다 먼저 한다. 신규 명령만 최초 대상을 해소하고 그 값을 기록한다.
  재시도 계약은 [ADR-0057](0057-command-identity-for-mutation-retries.md)을 따른다.
- 한 transaction이 expected revision·events·command 기록·effect 의무를 함께 확정한다. 실패하면 메모리 적용과 외부 실행을 시작하지 않는다.
- stream 단위는 엔진이다. 한 stream에는 활성 writer 하나만 있고 revision은 단조 증가한다. stream 사이의 전체 도착 순서는 보장하지 않는다.
  여러 엔진을 바꾸는 명령은 하나의 transaction으로 확정하고, 모두 staging에 적용한 뒤 하나의 공개 barrier를 넘긴다.
  여러 엔진을 읽는 조회는 batch ID와 revision vector가 같은 cut을 읽는다.
- `evolve`는 이벤트만으로 결정되며 디스크·OS·현재 포커스를 읽지 않는다. 시간·CWD·설정·registry 값과 예약 ID는 `decide`의 고정 입력으로 받는다.
- GUI의 Immediate/Domain 처리 순서와 redraw coalescing은 화면 일정이며 도메인 순서가 아니다.
- 원격의 다른 서버는 자기 stream의 writer다. client는 명령을 보내고 확정 결과를 mirror에 반영한다. 호스트 간 원자적 commit은 하지 않는다.

### 실행과 복원

- 생성은 준비 operation과 예약 ID를 먼저 확정하고, kind별 자원 준비가 끝난 뒤 구조 공개와 완료를 확정한다.
  commit 전에 PTY를 만드는 중간 상태를 운영 경로에 남기지 않는다.
- 닫기는 삭제와 정리 의무를 함께 확정한다. replay가 PTY drop을 다시 실행하지 않고 미완료 정리만 추적한다.
- effect는 Pending·Running·Deferred·성공·실패·Cancelled·Superseded·Uncertain을 구분한다. 결과가 불명확한 raw 입력이나 셸 명령은 자동으로 다시 보내지 않는다.
  영속 기록이 외부 OS 작업의 정확히 한 번 실행을 보장한다고 주장하지 않는다.
- 로그 재생은 저장 자료를 순수하게 재구성하며 PTY·네트워크·과거 사용자 입력·hook을 실행하지 않는다.
- 재시작 복원은 특정 View에 속했던 workspace/pane/tab/surface와 surface 저장 상태를 복원하는 것이다.
  재구성한 자료를 kind별 복원에 넘기고, terminal의 PTY 초기화·restore_command·scrollback 주입·즉시/지연 생성은 기존 surface 복원 로직이 수행한다.
  별도의 앱 수준 PTY 자동 복구 정책을 만들지 않는다. 같은 surface activation은 하나의 claim으로 합류하고 정상 respawn은 새 generation을 쓴다.
- 기존 레이아웃은 최초 한 번 가져온다. 이관 marker와 초기 event batch를 함께 확정하고, 실패하면 원본 파일을 보존한다.
  저널 활성화 뒤에는 오래된 레이아웃을 다시 원본으로 가져오지 않으며 호환 레이아웃은 export로만 만든다.
- 저장소가 요구 내구성을 만족하지 못하면 새 도메인 변경을 받지 않는다. 조용히 in-memory로 대체하지 않는다.

기존 IPC·CLI의 메서드·인자·오류 의미·응답과 저장 파일 읽기는 유지한다. revision·journal 보장은 capability로 협상한 선택적 확장으로만 제공하고,
새 event schema를 원격 wire schema로 직접 노출하지 않는다.

## Consequences

범위 안의 구조는 같은 기록에서 외부 자원 없이 같은 모델로 재생된다. 응답 유실·crash 뒤의 재시도와 미완료 생성·정리를
기록으로 판단할 수 있다. 모든 진입점이 한 경계를 쓰므로 검증·실패 문구·후속 처리가 갈라지지 않는다.

비용도 크다. 모든 writer를 찾아 옮겨야 하며, 한 writer라도 직접 변경을 계속하면 그 범위는 활성화할 수 없다.
저장 형식이 바뀌면 옛 바이너리로 되돌리는 일은 코드 revert와 다르다. 새 저널을 읽지 못하는 바이너리가 최신 상태를 덮어쓰지 않도록
version·소유 검사가 필요하다. 영속 commit이 추가되어 구조 변경의 지연이 늘 수 있으므로 UI thread가 디스크 대기를 하지 않게 해야 한다.

터미널 화면 전체, plugin 내부 상태, 설정·승인·작업 데이터는 이 기록으로 복구되지 않는다. 문서와 응답에서 적용 범위를 과장하지 않는다.

## Alternatives Considered

- Intent 큐의 위치만 옮기는 안: IPC 동기 경로·plugin·원격·시스템 writer가 기록 밖에 남는다.
- 현재 `CoreEvent`를 그대로 영속하는 안: 확정 사실과 후속 요청이 섞여 replay가 외부 실행을 다시 일으킨다.
- 레이아웃 snapshot과 저널을 둘 다 원본으로 두는 안: 두 원본이 갈라졌을 때 어느 쪽이 맞는지 판단할 수 없다.
- 모든 서비스 데이터를 하나의 저널로 합치는 안: 기존 저장소의 수명·권한·보존 정책과 맞지 않고 전환 범위가 끝나지 않는다.
- plugin EventBus를 저장소로 확장하는 안: 통지 링은 손실을 허용하는 소비자 계약이고 확정 순서·transaction을 제공하지 않는다.
- 객체마다 thread를 두는 안: 순서 보장이 어려워지고 이득이 없다. 책임 분리를 먼저 하고 thread 배치는 측정 뒤 정한다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 범위 밖 서비스(작업·훅·설정)를 같은 저널에 넣어야 하는 요구가 생기면, 실행 attempt·결과 관측·중복 계약을 먼저 설계하고 범위를 다시 정한다.
- 여러 호스트가 같은 stream을 쓰거나 호스트 간 원자적 변경이 필요해지면 writer·transaction 모델을 다시 정한다.
- 범위 안에 CommandExecutor를 거치지 않는 writer가 남아 있으면 해당 범위의 저널 활성화를 보류한다. writer 명부와 실제 mutation 호출부를 대조해 확인한다.

### 실행 결과로 확인

- 구조 명령 commit 지연이 사용자 입력 응답에 보이면 transaction 묶음과 저장 위치를 다시 본다. 측정은 commit 경과 시간과 큐 압력으로 한다.
- IPC·원격 forward·GUI 경로의 실패 문구 일치, 기존 문구 호환, GUI·헤드리스 두 빌드의 후속 처리를 각각 검증한다. 어긋나면 공용 경계 밖 경로가 남았는지 먼저 찾는다.
- 강제 종료 시험(migration·commit·effect·restore 경계)에서 중복 생성이나 누락 정리가 나오면 activation claim과 effect 상태 전이를 다시 본다.

## References

- [ADR-0054](0054-app-core-view-layers-and-state-ownership.md) · [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md) · [ADR-0057](0057-command-identity-for-mutation-retries.md) · [ADR-0060](0060-terminal-and-pty-separation.md) · [ADR-0061](0061-external-remote-module-and-attach-sync.md)
- [ADR-0063](0063-event-store-storage-fencing-and-effect-states.md) — `tasty-event-store`의 payload 저장·writer 잠금·effect 전이·schema 버전 결정
- [ADR-0033](0033-event-feed-delivery.md) — 통지용 이벤트 피드는 이 저장소와 별개다
- [ADR-0010](0010-storage-failure-reporting.md) — 기존 state.db·memory.db의 내구성 정책은 바뀌지 않는다
- 현재 흐름: [동작 처리 흐름](../design/flows/action-dispatch.md), [레이아웃 저장](../features/layout-persistence/index.md)
- 현재 구현: `src/intent.rs`, `src/core/intent.rs`, `src/app/dispatch/intents.rs`, `src/core/layout_persistence`, `crates/tasty-host-plugin/src/event_bus.rs`.
