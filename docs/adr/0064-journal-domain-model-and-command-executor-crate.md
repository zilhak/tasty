# ADR-0064: 저널 도메인 모델과 CommandExecutor는 새 crate `tasty-domain`에 새로 작성한다

- **Status**: Accepted — 구현 상태: 미이행(crate 작성 중). 제품 경로 연결은 root 배선 단계에서 한다
- **Date**: 2026-09-30
- **Tags**: architecture, crates, domain, event-sourcing, commands
- **Group**: foundation

## Context

[ADR-0055](0055-structural-domain-event-sourcing.md)는 구조 도메인의 원본을 확정 이벤트로 정하고
decide→commit→apply→응답을 CommandExecutor가, 순수 `decide`·`evolve`를 Core가 맡게 했다.
[ADR-0063](0063-event-store-storage-fencing-and-effect-states.md)의 `tasty-event-store`는 저장 계약을 구현했지만 도메인 타입을 모른다.
이벤트·effect·snapshot 내용은 type tag·schema version·바이트로만 저장하고 해석은 호출자의 codec에 맡긴다.
따라서 저장 계약을 도메인 수준에서 검증하려면(전체 replay와 snapshot+tail의 결과 비교, commit 실패 시 메모리 상태 불변,
같은 명령 재시도 시 decide 생략) 도메인 이벤트·모델·`evolve`와 최소 CommandExecutor가 먼저 있어야 한다.

[ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)은 이 역할을 새 `tasty-core`에 두기로 했지만,
그 추출은 `tasty-model`의 실행 결합을 걷어낸 뒤 시점을 다시 판단하는 것으로 남아 있다.
현재 도메인 코드인 `src/core`는 CoreState와 PTY·훅·작업 실행을 함께 들고 있어 그대로 옮길 수 없다.
저장 계약 검증을 `tasty-core` 추출까지 미루면 저장소는 계속 제품 경로 밖에서 장난감 모델로만 시험된다.

## Decision

저널 전용 구조 도메인을 새 crate `tasty-domain`에 새 코드로 작성한다. `src/core`를 옮기지 않는다.

- `tasty-domain`이 소유하는 것: 저널 전용 구조 모델(JournalModel), 도메인 이벤트(DomainEvent), 이벤트·snapshot codec,
  batch 단위의 순수 `evolve`, Decider trait, Decider에 대해 generic한 CommandExecutor.
  첫 이벤트 범위는 workspace·category·pane·tab·surface의 생성·분할·이동·이름 변경·닫기와 metadata 갱신이다.
- 의존: 저장 계약인 `tasty-event-store`만 의존한다. `tasty-model`·root·GUI·PTY 계층을 의존하지 않고, 필요한 ID·값 타입은 이 crate에 새로 둔다.
  `tasty-event-store`는 도메인을 모르므로 방향은 `tasty-domain` → `tasty-event-store`다.
  ADR-0056이 계획한 "event-store가 도메인 타입을 의존할 수 있다"는 방향은 쓰지 않는다. 저장 형식의 type tag·schema version 해석은 이 crate의 codec이 맡는다.
- codec은 모르는 type tag나 schema version을 만나면 명시 오류로 재구성을 멈춘다. 건너뛰고 계속하지 않는다.
- CommandExecutor는 Decider의 State·Command·Event·결정 문맥에 generic하다. 명령 identity 조회(대상 해소보다 먼저, [ADR-0057](0057-command-identity-for-mutation-retries.md)),
  decide, 한 transaction의 commit, commit 성공 뒤의 메모리 `evolve`, 응답 순서를 구현한다. 호출자 권한 확인·wire 응답 조립·EngineSession·EffectRunner 같은
  제품 쪽 조립은 ADR-0056대로 root runtime에 둔다.
- 제품 경로에 연결하지 않는다. root의 어떤 부팅·IPC·GUI 경로도 이 crate를 쓰지 않으며, 연결은 root 배선 단계에서 따로 한다.
  그 전까지 JournalModel은 CoreState와 동시에 원본이 아니다. 현재 구조의 원본은 계속 메모리 CoreState와 레이아웃 snapshot이다.
- 로그 보존·정리와 journal 사이의 payload 복사는 이 crate를 만드는 단계에 넣지 않고 journal을 복원 원본으로 전환하는 단계에서 설계한다.
  두 항목은 ADR-0063의 미이행 목록에 그대로 남는다.

## Consequences

저장 계약을 도메인 이벤트·실제 journal로 `-p tasty-domain` 단위에서 검증할 수 있고, root 빌드와 교집합 없이 병렬로 작성할 수 있다.
도메인 crate가 GUI·PTY·SQL을 직접 참조하면 컴파일 오류가 나며, SQL 의존은 `tasty-event-store`를 거친다.

JournalModel은 현재 CoreState와 별개인 두 번째 구조 모델이다. 제품에 연결하기 전까지 두 모델의 관계는 importer와 비교 시험으로만 확인되며,
연결 단계에서 어느 쪽이 원본인지 전환하는 절차가 필요하다. 이 사이에 CoreState 쪽 구조 규칙이 바뀌면 JournalModel에도 반영해야 한다.

도메인 쪽 이름이 `tasty-core`(ADR-0056)와 `tasty-domain` 둘로 나뉠 수 있다. 추출 판단 때 정리하지 않으면 같은 역할의 crate가 둘이 된다.
crate 목록 문서·README·가드의 crate 수 갱신이 함께 필요하다.

## Alternatives Considered

- root 안의 새 journal 모듈에 작성하는 안: 새 파일뿐이라 충돌은 작지만 root 컴파일 단위에 묶여 시험을 root 전체 lib 시험으로만 돌릴 수 있다.
  도메인을 root 밖에 두는 ADR-0056의 방향과도 어긋난다.
- 지금 `tasty-core`를 추출하는 안: `src/core`의 실행 결합 정리가 선행 조건인데 아직 끝나지 않았다. 저장 계약 검증이 그 정리에 묶여 늦어지고,
  옮기는 과정에서 기존 CoreState 경로를 함께 바꾸게 된다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- `tasty-core` 추출 시점을 판단할 때 `tasty-domain`과 합칠지, 이름을 바꿀지, 두 crate로 둘지를 함께 정한다. 추출 결과 도메인 Command/Event 소유자가 둘이 되면 하나로 모은다.
- root 배선 단계에서 CommandExecutor의 generic 부분과 root runtime 조립의 경계가 맞지 않으면(예: 권한·대상 해소 문맥이 Decider 문맥에 들어가야 하면) 이 배치를 다시 본다.
- `tasty-domain`이 `tasty-event-store` 외의 실행 계층(PTY·GUI·root)을 의존해야 하는 요구가 생기면 이 경계를 다시 정한다. `cargo tree -p tasty-domain --edges normal`로 확인한다.

### 실행 결과로 확인

- 제품 연결 전에 importer와 비교 시험에서 JournalModel과 CoreState의 구조가 다르게 나오는 사례가 반복되면 별도 모델을 유지하는 비용을 다시 판단한다.

## References

- [ADR-0055](0055-structural-domain-event-sourcing.md) — 객체와 책임, 확정 경계
- [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md) — crate 배치와 `tasty-core` 추출 조건
- [ADR-0057](0057-command-identity-for-mutation-retries.md) · [ADR-0063](0063-event-store-storage-fencing-and-effect-states.md)
- 현재 구현: `crates/tasty-event-store/src/lib.rs`(도메인을 모르는 저장 계약)
