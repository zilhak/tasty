# ADR-0073: 닫기의 자원 receipt 대기는 관측을 멈추지 않는다

- **Status**: Accepted
- **Date**: 2026-10-07
- **Tags**: event-sourcing, effects, close, retirement, ipc, plugin
- **Group**: foundation

## Context

닫기는 구조 변경을 확정해 게시하고, 원 실행 자원(PTY·plugin surface)을 회수한 뒤 응답한다([닫기와 실행 자원 회수](../architecture/close-sequence.md)).
App은 journal 사실을 게시하는 동안 관측을 멈춘다. 이 동안 IPC·창 입력을 보류하고 plugin pump도 돌리지 않는다. 다른 관측자가 게시 중간 상태를 보지 않게 하는 장치다([ADR-0065](0065-journal-source-and-core-state-projection.md)의 게시 순서).

이 일시정지가 닫기 정리의 receipt 대기에도 걸려 있었다. 이 단계에서는 이미 구조가 게시됐고 자원 owner는 `ResourceRetirement`가 쥐고 있으며 plugin·PTY의 회수 증거만 기다린다. plugin 프로세스가 멈추면 이 대기가 receipt 시한(5초) 동안 이어졌다.
- 그동안 `list info` 같은 무관한 IPC도 답하지 못했다.
- 멈춘 plugin을 회수하려고 보낸 `plugin disable`도 시한 뒤로 미뤄졌다. 그래서 회수 증거로 닫기를 확정할 수 없었고, 닫기는 `Uncertain`으로 끝났다.

## Decision

닫기 정리의 `Running` 단계(자원 receipt 대기)에서는 관측을 멈추지 않는다. journal 쓰기와 게시가 걸린 짧은 단계만 관측을 멈춘다.

- 정리 항목: `Claim`(실행 권한 확정)과 `Finish`(결과 확정·metadata 정리·닫힘 이벤트 적재)만 일시정지 대상이다. `Running`·`Reconcile`·`Reconciled`는 대상이 아니다(`src/app/journal/resource_cleanup.rs`의 `cleanup_pauses_observation`).
- 닫기 명령: 커밋 전에는 일시정지한다. 커밋 뒤 그 명령이 준비한 operation이 **모두** `Running`이면 일시정지를 푼다. 하나라도 `Finish` 이후로 넘어가면 응답까지 다시 일시정지한다. 그래서 닫기의 최종 사실·응답이 뒤의 입력보다 먼저 나간다. operation을 알 수 없으면 지금처럼 일시정지한다(`src/app/journal/commands.rs`의 `close_pauses`). 교체(replacement)는 바꾸지 않는다.
- `Claim`이 `Running`으로 바뀌는 즉시 plugin의 회수 게시를 적용한다(`poll_publication_retirements`). 관측이 재개된 첫 턴은 보류한 입력을 pump보다 먼저 실행한다. 이 순서 때문에 보류된 enable이 파괴 중인 surface를 새 프로세스에 다시 게시하는 일을 막는다.
- 앱 종료(GUI의 `SavingLayout`, headless의 종료 마무리)는 `Running` 정리도 기다린다(`shutdown_waits_for_publication`). 이 대기 동안 plugin pump가 돌지 않으므로 종료 단계가 회수 응답을 직접 처리한다. 상한은 receipt 시한이다. 그래서 종료가 걸린 닫기도 응답을 한 번 보내고, 종료 전에 결과(시한이 지나면 `Uncertain`)를 기록한다.

같은 자원을 다루는 다른 명령에 대한 보장은 다음과 같다.

| 상황 | 보장 | 근거 |
|---|---|---|
| 같은 surface·workspace id | 회수 중인 id는 다시 쓰이지 않는다 | id 예약은 단조 증가하며 되감지 않는다. 닫은 항목 복원도 새 id를 받는다. metadata 정리는 id가 다시 살아 있으면 거절한다 |
| 같은 plugin disable·재기동 | 먼저 보낸 파괴 요청은 원 세대 회수로 확정된다. 회수 중 surface는 새 프로세스에 다시 게시되지 않는다 | 세대 회수 확정과 회수 게시 즉시 적용 |
| 엔진 은퇴 | receipt가 남은 엔진은 해제하지 않는다 | 엔진 해제는 정리 항목·보류 회수가 빌 때까지 기다린다 |
| 같은 워크스페이스의 다른 닫기 | 같은 자원을 두 번 회수하지 않는다 | 회수 대상은 이미 구조에서 빠졌다. 뒤 명령의 `Claim`·`Finish`는 여전히 일시정지한다 |
| 사용자 창 입력 | 입력은 닫기가 반영된 구조를 대상으로 바로 처리된다 | 닫힌 surface는 이미 View·구조에 없다. 일시정지 단계의 입력 보류·재검사는 그대로다 |
| 앱 종료 | 종료가 걸린 닫기도 응답을 한 번 보내고, 종료 전에 결과를 기록한다 | 종료는 `Running` 정리를 기다리고 그동안 회수 응답을 직접 처리한다. 상한은 receipt 시한이다 |
| 레이아웃 저장·surface capture·preset capture | 닫히는 surface를 저장하거나 되살리지 않는다 | 닫히는 surface는 구조와 runtime 표에서 빠져 `ResourceRetirement`만 쥐고 있다. 저장·capture는 닫기가 반영된 상태를 읽는다 |
| 원격 attach 갱신·해석 대기 요청 | 클라이언트가 닫기 중간 상태를 받지 않는다 | 구조 변경은 이미 확정·게시됐다. 클라이언트는 닫기가 반영된 구조를 받는다. 닫힘 이벤트는 결과 확정 뒤에 나간다 |

## Consequences

- 멈춘 plugin의 닫기를 기다리는 동안에도 무관한 IPC가 평상시 지연으로 답한다. plugin pump와 헬스체크도 돈다.
- 닫기 대기 중 보낸 `plugin disable`이 바로 처리된다. 원 프로세스 회수가 시한 안에 닫기를 성공으로 확정한다.
- `Running` 동안 관측자는 "구조에서는 빠졌지만 응답은 아직"인 상태를 볼 수 있다. 목록 조회에서 닫히는 surface는 이미 보이지 않는다. 닫힘 이벤트와 닫기 응답은 `Finish` 뒤에 나간다.
- `Running` 동안 사용자가 다른 surface를 클릭하거나 입력하면 바로 처리된다. 이전에는 닫기가 끝날 때까지 보류됐다가 버려질 수 있었다.
- 앱 종료는 멈춘 plugin의 닫기를 receipt 시한까지 기다릴 수 있다.
- `Running` 동안 닫히는 surface의 metadata를 쓰면 바로 성공한다. 그 값은 `Finish`의 scope 정리로 지워진다.
- 닫기 명령은 준비한 operation id를 응답 진행 기록에서 읽어 보관한다. 유지 비용은 이 판정과 정리 단계 판정의 일치다. `close_pause_tests`와 `resource_cleanup::tests`가 단계별 판정을 검사한다.

## Alternatives Considered

- **읽기 전용 IPC만 보류에서 뺀다.** 목록 조회는 풀리지만 `plugin disable`은 쓰기라 여전히 시한 뒤로 미뤄진다. 그래서 멈춘 plugin의 닫기를 성공으로 확정하지 못한다. 메서드 분류 목록을 하나 더 관리해야 하고, 읽기가 게시 전 상태를 볼 수 있는지도 따로 정의해야 한다.
- **닫기를 비동기 완료로 바꾼다.** 구조 확정 뒤 바로 수락 응답을 주고 회수 결과는 별도 조회·이벤트로 알린다. 막히는 시간은 가장 짧다. 그러나 `closed:true`의 의미와 `-32068` 재시도 규약이라는 공개 IPC 계약을 바꾼다. CLI·plugin·문서를 함께 바꿔야 한다. 회수 증거가 응답에 묶여야 한다는 [ADR-0063](0063-event-store-storage-fencing-and-effect-states.md)의 effect 상태 계약과도 맞지 않는다.
- **설계를 유지하고 receipt 시한만 줄인다.** 막히는 상한은 줄지만 0이 되지 않는다. 시한이 짧아지면 정상적으로 느린 plugin의 회수도 `Uncertain`으로 끝난다.

## Reconsideration Triggers

코드와 설정에서 확인:
- 정리 `Running` 단계에 journal 쓰기나 구조 게시가 추가되면 그 단계도 일시정지해야 한다. `answer_resource_cleanup`과 `poll_resource_cleanup`이 `Running`에서 worker에 쓰기를 보내는지 확인한다.
- id 예약이 재사용을 허용하게 바뀌면 같은 id 재사용 보장이 사라진다(`EventStore::reserve_ids`).
- 관측 재개 첫 턴의 순서(보류 입력 → plugin pump)가 바뀌거나 enable 경로가 회수 게시보다 먼저 surface를 재게시하게 되면 즉시 적용의 필요를 다시 판단한다.
- 종료 단계가 plugin pump를 돌리게 되면 `SavingLayout`의 직접 회수 처리가 필요한지 다시 판단한다.

실행 결과로 확인:
- plugin 프로세스를 SIGSTOP한 상태에서 surface를 닫는 동안 `list info` 지연과, 0.5초 뒤 `plugin disable`을 보냈을 때 닫기가 성공으로 끝나는지를 격리 debug 인스턴스에서 잰다. 지연이 receipt 시한 수준으로 돌아가면 다시 검토한다.

## References

- [닫기와 실행 자원 회수](../architecture/close-sequence.md) — 현재 닫기 단계와 관측 범위.
- [ADR-0055](0055-structural-domain-event-sourcing.md) — 구조 명령의 확정과 응답 순서. 이 결정은 응답 순서를 바꾸지 않는다.
- [ADR-0063](0063-event-store-storage-fencing-and-effect-states.md) — effect 상태와 receipt 계약. 상태 전이는 바꾸지 않고 관측 일시정지 범위만 줄인다.
- [ADR-0065](0065-journal-source-and-core-state-projection.md) — 확정 batch 게시와 관측 보류. 구조 게시 단계의 보류는 그대로다.
- `src/app/journal/resource_cleanup.rs`, `src/app/journal/commands.rs`, `src/app/publication_input.rs`.
