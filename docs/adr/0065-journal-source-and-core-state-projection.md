# ADR-0065: 구조 journal이 원본이고 CoreState 트리는 확정 이벤트로 갱신하는 live projection이다

- **Status**: Accepted — 구현 상태: 이행 중. App의 초기 엔진 구성은 journal worker·확정 projection·실제 자원 설치에 연결했다. 선택한 복원 자료도 같은 activation 경계를 사용한다. 일반 구조 producer와 legacy import·복구 경계가 아직 이행 중이므로 엔진 단위 활성화 조건을 모두 충족한 제품 끝점은 아니다
- **Date**: 2026-09-30
- **Tags**: architecture, event-sourcing, domain, projection, migration
- **Group**: foundation

## Context

[ADR-0055](0055-structural-domain-event-sourcing.md)는 적용 범위 안의 구조 원본을 확정 이벤트로 정했고,
[ADR-0064](0064-journal-domain-model-crate.md)는 그 이벤트를 재생하는 구조 모델(JournalModel)을 `tasty-domain`에 새로 작성했다.
0064는 제품에 연결하기 전까지 JournalModel이 CoreState와 동시에 원본이 아니라고만 정했고, 연결한 뒤 두 모델의 관계와 전환 절차는 정하지 않았다.

두 모델은 모양이 다르다. JournalModel(`crates/tasty-domain/src/model.rs`)은 ID 키 map에 이름·소속·분할 트리·kind·자료 참조·metadata만 담는다.
결정 당시 `tasty-model`의 CoreState 트리는 surface 실행 인스턴스(`Box<dyn Surface>`), 사용자 선택 필드(`Workspace.focused_pane`·`Pane.active_tab`·`Tab.focused_surface`),
Terminal에서 계산한 파생 캐시(`Tab.osc_title`·`cached_display_name`), 분할 트리의 호환 hint(`focus_second`)를 함께 담는다.
IPC 응답과 GUI는 CoreState 트리를 읽는다.

## Decision

### 원본과 projection

- 구조 journal을 활성화한 엔진에서는 JournalModel이 [ADR-0055](0055-structural-domain-event-sourcing.md) 적용 범위의 원본이다.
- CoreState 트리는 commit된 batch를 적용받아 갱신되는 live projection이다. JournalModel에 없는 실행 인스턴스를 가지며, IPC 응답과 GUI는 계속 이 트리에서 값을 만든다.
- 활성화한 엔진에서 로컬 구조 트리의 writer는 projection 적용기 하나뿐이다. CommandExecutor가 commit에 성공한 batch만 적용기로 넘기며, 다른 경로가 트리를 직접 바꾸지 않는다.
  원격 mirror 구조는 로컬 트리와 다른 필드에 있고 쓰는 창구도 다르다([ADR-0061](0061-external-remote-module-and-attach-sync.md)).
- 이벤트에 넣지 않는 값:
  - 사용자 선택은 [ADR-0059](0059-id-targets-and-view-owned-selection.md)에 따라 View 소유다. 선택 필드가 아직 CoreState에 있는 동안에는 적용기가 0059의 보정 규칙대로 유지한다.
  - `focus_second`는 적용기가 기존 hint 규칙대로 채운다.
  - `osc_title`과 표시 이름 같은 Terminal 파생값은 적용 뒤 기존 계산으로 다시 만든다.

### 전환 절차

- shadow 기간에는 기존 경로가 원본이다. 같은 명령을 CoreState와 JournalModel에 모두 적용하고 두 결과의 구조 digest를 비교한다.
  이 기간의 journal 기록은 비교에만 쓰고 복원에는 쓰지 않는다. digest가 다르면 그 범위를 활성화하지 않는다.
- 활성화는 엔진 단위로 차례로 한다. 엔진 하나가 journal의 stream 하나다([ADR-0063](0063-event-store-storage-fencing-and-effect-states.md)의 ID 예약 절).
  새로 만든 엔진을 먼저 켜고, 레이아웃 슬롯에서 복원한 엔진은 importer의 이관 marker를 확정한 뒤 켠다.
  mirror 구조가 전용 필드로 옮겨지기 전에는 mirror workspace를 가진 엔진을 켜지 않는다.
- 활성화 조건은 0055와 같다. 해당 엔진의 범위 안 writer가 모두 CommandExecutor로 합류하고, 옛 writer와 새 writer가 같은 대상을 섞어 쓰지 않아야 한다.

### 현재 이행 경계

`src/app/journal.rs`가 데이터 홈의 worker 하나와 엔진별 비동기 continuation을 연결한다.
worker의 순수 구조 모델을 초기 논리 projection과 확정 batch 적용에 사용하며,
App이 별도 가변 JournalModel 원본을 유지하지 않는다. 준비 요청·완료 채널과 복원 읽기 개수,
App 한 회의 완료 처리량은 제한한다.

생성·변환은 Pending operation과 outbox를 확정하고 claim한 뒤 후보를 만든다.
준비 성공 다음 commit은 설치 권한·옛 owner 정리 의무만 확정한다. 외부 게시·설치와 정확한
옛 PTY 회수 뒤 최종 commit이 구조 변경·Ready·원 요청 완료를 함께 확정하고 공개한다.
설치 전 kind 철회·등록 교체는 후보를 폐기하고 기존 인스턴스를 유지한다.
외부 게시 이후의 불명 결과는 이행 중인 Recovery 경계에서 대조해야 하며, 알려진 실패로
바꿔 자동 재실행하지 않는다.

표시면은 첫 capture 전에도 생성 자료 참조를 유지한다. 복원은 snapshot을 우선하고,
없으면 generic 생성 params/CWD를 사용한다. terminal은 현재 셸 설정과 저장된 명시적
복원 명령을 사용하며 과거 실행 인자·입력을 자동 재전송하지 않는다.
복원 capture는 기존 DataRef로 읽고, 같은 자료를 준비 요청에 복사해 다시 저장하지 않는다.
선택되지 않은 terminal과 아직 등록되지 않은 kind의 지연 활성화 및 일반 명령 진입점은
계속 이행 중이다. 이 상태를 완전한 writer 단일화나 장애 복구 완료로 해석하지 않는다.

## Consequences

기존 JournalModel·`evolve`·importer·CommandExecutor를 그대로 쓴다. CoreState를 먼저 순수 모델로 바꾸지 않아도 전환을 시작할 수 있다.
IPC 응답 형식은 CoreState에서 계속 만들므로 바뀌지 않는다. 선택이 이벤트 밖에 있어 replay가 사용자 포커스를 다시 실행하지 않는다.

이벤트를 CoreState에 적용하는 대응 코드를 따로 유지해야 한다. 적용기 결함은 digest 비교로만 드러난다.
0064의 재검토 조건인 두 모델 유지 비용 판단도 계속 해야 한다.
활성화한 엔진과 아직 켜지 않은 엔진이 한 프로세스에 함께 있는 기간이 생긴다. 엔진마다 어느 쪽이 원본인지 명확해야 한다.

## Alternatives Considered

- CoreState 트리를 journal 모델로 개조하는 단일 모델안: 트리에서 선택·파생 캐시·실행 인스턴스를 먼저 빼야 하므로 전환 시작이 그 작업 뒤로 밀린다.
  비교할 두 번째 모델이 없어 전환 중 안전망도 약해진다.
- 명령을 JournalModel과 CoreState에 각각 독립 적용하는 안: 원본이 둘이 된다. 0055가 기각한 상태가 굳어지며, 두 모델이 갈라지면 어느 쪽이 맞는지 판정할 수 없다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 선택 필드가 View로 옮겨지고 surface 실행 인스턴스가 트리 밖으로 분리돼 CoreState 트리가 JournalModel과 같은 모양이 되면 두 모델을 합칠지 다시 본다.
- projection 적용기 밖의 로컬 구조 트리 writer가 남아 있으면 그 엔진을 활성화하지 않는다. writer 명부와 실제 mutation 호출부를 대조해 확인한다.

### 실행 결과로 확인

- shadow 비교에서 digest 불일치가 반복되면 적용기와 `evolve`의 규칙 차이를 먼저 찾고, 두 모델 유지 비용을 다시 판단한다.

## References

- [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0059](0059-id-targets-and-view-owned-selection.md) · [ADR-0061](0061-external-remote-module-and-attach-sync.md) · [ADR-0063](0063-event-store-storage-fencing-and-effect-states.md)
- [ADR-0064](0064-journal-domain-model-crate.md) — JournalModel과 시험 전용 executor
- 현재 구현: `crates/tasty-domain/src/model.rs`(JournalModel), `crates/tasty-model/src/workspace.rs`·`crates/tasty-model/src/tab.rs`(CoreState 트리), `src/runtime/command_executor.rs`, `src/core/layout_persistence/import.rs`(importer).
