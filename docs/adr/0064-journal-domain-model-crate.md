# ADR-0064: 저널 도메인 모델은 `tasty-core` 추출 전에 순수 도메인 crate `tasty-domain`에 새로 작성한다

- **Status**: Accepted — 구현 상태: `tasty-domain` crate와 root `src/runtime` 모듈(generic CommandExecutor, 저장 batch 변환, 전체 replay와 snapshot+tail 재구성)이 있다. 둘 다 시험 전용이며 제품 경로에는 연결되지 않았다. 미이행: 제품 배선, 저널 ID 공간과 runtime ID 공간의 구분(재검토 조건 참조)
- **Date**: 2026-09-30
- **Tags**: architecture, crates, domain, event-sourcing, commands
- **Group**: foundation

## Context

[ADR-0055](0055-structural-domain-event-sourcing.md)는 구조 도메인의 원본을 확정 이벤트로 정하고
decide→commit→apply→응답을 CommandExecutor가, 순수 `decide`·`evolve`를 Core가 맡게 했다.
[ADR-0063](0063-event-store-storage-fencing-and-effect-states.md)의 `tasty-event-store`는 저장 계약을 구현했지만 도메인 타입을 모른다.
이벤트·effect·snapshot 내용은 type tag·schema version·바이트의 봉투로만 저장하고 해석은 호출자에게 맡긴다.
따라서 저장 계약을 도메인 수준에서 검증하려면(전체 replay와 snapshot+tail의 결과 비교, commit 실패 시 메모리 상태 불변,
같은 명령 재시도 시 decide 생략) 도메인 이벤트·모델·`evolve`와 최소 CommandExecutor가 먼저 있어야 한다.

[ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)은 이 역할을 새 `tasty-core`에 두기로 했다.
`tasty-core`는 기존 `src/core`에서 도메인 부분을 추출하는 작업이며, 그 시점은 `tasty-model`의 실행 결합을 걷어낸 뒤 다시 판단하는 것으로 남아 있다.
현재 `src/core`는 CoreState와 PTY·훅·작업 실행을 함께 들고 있어 그대로 옮길 수 없다.
저장 계약 검증을 `tasty-core` 추출까지 미루면 저장소는 계속 제품 경로 밖에서 장난감 모델로만 시험된다.

## Decision

저널 전용 구조 도메인을 순수 도메인 crate `tasty-domain`에 새 코드로 작성한다. `src/core`를 옮기지 않는다.

- `tasty-domain`이 소유하는 것: 도메인 이벤트(DomainEvent), 저널 전용 구조 모델(JournalModel), batch 단위의 순수 `evolve`, Decider trait,
  도메인 이벤트 payload codec(type tag·schema version과 바이트 사이의 변환).
  첫 이벤트 범위는 workspace·category·pane·tab·surface의 생성·분할·이동·이름 변경·닫기와 metadata 갱신이다.
- 의존: 값·ID 타입을 재사용하기 위한 `tasty-model`뿐이다. `tasty-event-store`·root·GUI·PTY·SQL 계층을 의존하지 않는다.
  저장 봉투, 저장 형식 버전, migration은 EventStore의 소관이다.
- codec은 모르는 type tag나 schema version을 만나면 명시 오류로 재구성을 멈춘다. 건너뛰고 계속하지 않는다.
  도메인 payload의 schema version을 올릴 때 옛 payload를 새 형식으로 바꾸는 변환(upcast)도 이 codec이 맡는다. EventStore는 봉투의 저장 형식 버전과 migration만 맡는다.
- Decider에 대해 generic한 CommandExecutor와, 저장소의 batch(`StoredBatch`)와 도메인 batch 사이의 변환 어댑터는 root 내부 runtime 모듈에 둔다.
  명령 identity 조회(대상 해소보다 먼저, [ADR-0057](0057-command-identity-for-mutation-retries.md)), decide, 한 transaction의 commit,
  commit 성공 뒤의 메모리 `evolve`, 응답 순서를 그 모듈이 구현한다.
- 제품 경로에 연결하지 않는다. root의 어떤 부팅·IPC·GUI 경로도 이 모델을 쓰지 않으며, 연결은 root 배선 단계에서 따로 한다.
  그 전까지 JournalModel은 CoreState와 동시에 원본이 아니다. 현재 구조의 원본은 계속 메모리 CoreState와 레이아웃 snapshot이다.
- 로그 보존·정리와 journal 사이의 payload 복사는 이 crate를 만드는 단계에 넣지 않고 journal을 복원 원본으로 전환하는 단계에서 설계한다.
  두 항목은 ADR-0063의 미이행 목록에 그대로 남는다.

### ADR-0056과의 관계

이 결정은 ADR-0056의 배치 결정을 바꾸지 않는다. 0056 표에서 도메인 Command/Event와 순수 decide/evolve를 `tasty-core`에 두는 행은 `tasty-core` 추출 시점까지 발동하지 않으며, 추출할 때 `tasty-domain`과 합치거나 이름을 정리한다. 순수 구조 타입은 `tasty-model`에 두고 새 domain-types crate를 만들지 않는다는 배치(0056 표의 순수 구조 타입 행)와
`tasty-domain-types` 신설을 기각한 대안을 따라, ID와 분할 방향처럼 `tasty-model`에 같은 역할의 타입이 있으면 그 타입을 쓴다. 그 타입에 직렬화 형식이 없으면(`SplitDirection`) serde remote 정의를 이 crate에 둔다.
저널에만 필요한 값(revision·batch ID·비율 비트 표현 `Ratio`·자료 참조 `DataRef`·분할 트리 표현 `SplitTree`·ID 종류 `IdKind`)은 이 crate에 둔다.
`tasty-model`의 `PaneNode`·`SurfaceLayout`은 pane·surface 실행 인스턴스를 담고 비율을 `f32` 값으로 들고 있어 저널 값으로 쓸 수 없기 때문이다.
CommandExecutor를 root 내부 runtime 모듈에 둔다는 표의 행도 그대로 따른다.
도메인 crate와 `tasty-event-store`의 의존 방향은 0056이 금지한 쪽(도메인 → event-store)을 쓰지 않으며, 두 crate는 서로 의존하지 않는다.
0056이 EventStore에 둔 저장 형식 버전·codec·migration은 저장 봉투에 관한 것이고, `tasty-domain`의 codec은 그 봉투 안 바이트와 도메인 이벤트 사이의 변환만 맡는다.
이 codec은 명시적인 type tag·schema version을 쓰므로 도메인 타입 필드를 바꿔도 저널 직렬화가 자동으로 바뀌지 않는다는 0056의 요구를 지킨다.

## Consequences

저장 계약을 도메인 이벤트로 검증하는 순수 부분을 `-p tasty-domain` 단위에서 시험할 수 있고, root 빌드와 교집합 없이 병렬로 작성할 수 있다.
도메인 crate가 GUI·PTY·SQL을 참조하면 컴파일 오류가 난다. 저장소와 도메인을 함께 쓰는 시험(commit 실패 시 상태 불변, 같은 명령 재시도)은
root `src/runtime` 모듈의 시험에서 돈다.

JournalModel은 현재 CoreState와 별개인 두 번째 구조 모델이다. 제품에 연결하기 전까지 두 모델의 관계는 importer와 비교 시험으로만 확인되며,
연결 단계에서 어느 쪽이 원본인지 전환하는 절차가 필요하다. 이 사이에 CoreState 쪽 구조 규칙이 바뀌면 JournalModel에도 반영해야 한다.

도메인 쪽 crate 이름이 `tasty-core`와 `tasty-domain` 둘로 나뉠 수 있다. 추출 판단 때 정리하지 않으면 같은 역할의 crate가 둘이 된다.
crate 목록 문서·README·가드의 crate 수 갱신이 함께 필요하다.

## Alternatives Considered

- root 안의 새 journal 모듈에 작성하는 안: 새 파일뿐이라 충돌은 작지만 root 컴파일 단위에 묶여 시험을 root 전체 lib 시험으로만 돌릴 수 있다.
  도메인을 root 밖에 두는 ADR-0056의 방향과도 어긋난다.
- 지금 `tasty-core`를 추출하는 안: `src/core`의 실행 결합 정리가 선행 조건인데 아직 끝나지 않았다. 저장 계약 검증이 그 정리에 묶여 늦어지고,
  옮기는 과정에서 기존 CoreState 경로를 함께 바꾸게 된다.
- CommandExecutor와 저장 어댑터를 `tasty-domain`에 함께 두는 안: 도메인 crate가 `tasty-event-store`를 의존하게 되어 ADR-0056이 금지한 방향이 되고,
  executor를 root runtime에 둔다는 배치와도 어긋난다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- `tasty-core` 추출 시점을 판단할 때 `tasty-domain`과 합칠지, 이름을 바꿀지를 함께 정한다. 추출 결과 도메인 Command/Event 소유자가 둘이 되면 하나로 모은다.
- 제품 배선 전: 저널 ID와 runtime ID는 모두 `tasty-model`이 재수출하는 같은 `u32` 별칭이라 컴파일러가 두 공간을 구분하지 못한다. EventStore의 예약은 `u64` 범위를 내주지만 `IdSupplier`는 `u32`를 준다. 제품에 배선하기 전에 `u64`→`u32` 좁힘 규칙과, 공간 통합 또는 newtype 구분을 정한다.
- root 배선 단계에서 Decider 문맥에 권한·대상 해소 같은 root 전용 값이 들어가야 하면 Decider trait과 root runtime의 경계를 다시 본다.
- `tasty-domain`이 `tasty-model` 외의 저장·실행 계층(`tasty-event-store`·PTY·GUI·root)을 의존해야 하는 요구가 생기면 이 경계를 다시 정한다.
  `cargo tree -p tasty-domain --edges normal`로 확인한다.

### 실행 결과로 확인

- 제품 연결 전에 importer와 비교 시험에서 JournalModel과 CoreState의 구조가 다르게 나오는 사례가 반복되면 별도 모델을 유지하는 비용을 다시 판단한다.

## References

- [ADR-0055](0055-structural-domain-event-sourcing.md) — 객체와 책임, 확정 경계
- [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md) — crate 배치와 `tasty-core` 추출 조건
- [ADR-0057](0057-command-identity-for-mutation-retries.md) · [ADR-0063](0063-event-store-storage-fencing-and-effect-states.md)
- 현재 구현: `crates/tasty-domain/src/model.rs`·`crates/tasty-domain/src/ids.rs`(저널 모델·저널 전용 값·`IdSupplier`), `src/runtime/command_executor.rs`·`src/runtime/journal.rs`(executor·저장 batch 변환·replay·snapshot), `crates/tasty-event-store/src/lib.rs`(도메인을 모르는 저장 계약), `crates/tasty-model`(재사용할 값·ID 타입)
