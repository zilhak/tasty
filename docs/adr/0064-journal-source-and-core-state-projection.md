# ADR-0064: 구조 journal이 원본이고 CoreState 트리는 확정 이벤트로 갱신하는 live projection이다

- **Status**: Accepted — journal worker의 원본 모델과 확정 batch projection, 별도 실행 인스턴스, 명시 activation·retirement receipt가 구현돼 있다. View 복원 원본은 DB restore manifest와 연결된 checkpoint이며 sidecar는 최초 이관에만 사용한다. 타입/cfg·전체 writer 소비 및 crash/플랫폼 실행 검증은 구현 존재와 별개다.
- **Date**: 2026-09-30
- **Tags**: architecture, event-sourcing, domain, projection, migration
- **Group**: foundation

## Context

[ADR-0055](0055-structural-domain-event-sourcing.md)는 적용 범위 안의 구조 원본을 확정 이벤트로 정했고,
그 이벤트를 재생하는 구조 모델(JournalModel)은 `tasty-core`에 있다([ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)의 도메인 경계 절).
제품에 연결하기 전까지는 JournalModel을 CoreState와 동시에 원본으로 두지 않았고, 연결한 뒤 두 모델의 관계와 전환 절차가 이 ADR의 대상이다.

두 모델은 모양이 다르다. JournalModel(현재 `crates/tasty-core/src/model.rs`)은 ID 키 map에 이름·소속·분할 트리·kind·자료 참조·metadata만 담는다.
결정 당시 `tasty-model`의 CoreState 트리는 surface 실행 인스턴스(`Box<dyn Surface>`), 사용자 선택 필드(`Workspace.focused_pane`·`Pane.active_tab`·`Tab.focused_surface`),
Terminal에서 계산한 파생 캐시(`Tab.osc_title`·`cached_display_name`), 분할 트리의 호환 hint(`focus_second`)를 함께 담는다.
IPC 응답과 GUI는 CoreState 트리를 읽는다.

## Decision

### 원본과 projection

- 구조 journal을 활성화한 엔진에서는 JournalModel이 [ADR-0055](0055-structural-domain-event-sourcing.md) 적용 범위의 원본이다.
- CoreState 트리는 commit된 batch를 적용받아 갱신되는 live projection이다. 실행 인스턴스는 EngineRuntime으로 분리하고 트리에는 descriptor를 둔다. IPC 응답과 GUI는 계속 이 읽기 트리에서 값을 만든다.
- 활성화한 엔진에서 로컬 구조 트리의 writer는 projection 적용기 하나뿐이다. CommandExecutor가 commit에 성공한 batch만 적용기로 넘기며, 다른 경로가 트리를 직접 바꾸지 않는다.
  원격 mirror 구조는 로컬 트리와 다른 필드에 있고 쓰는 창구도 다르다([ADR-0061](0061-external-remote-module-and-attach-sync.md)).
- 사용자와 에이전트가 명시적으로 쓰는 속성은 JournalModel의 typed 필드와 전용 이벤트로 둔다. workspace의 subtitle·description·attach 매핑과 tab의 명시 이름이 여기에 든다.
  attach 매핑은 workspace와 원격 프로필·원격 workspace를 잇는 사용자 설정이며 `RemoteState`가 소유하는 runtime ID mapping과 다르다.
  generic metadata 이벤트(`MetadataSet`·`MetadataRemoved`)는 사용자 정의 키에만 쓰고 이 속성들을 예약 키로 넣지 않는다. legacy importer도 이 속성을 typed 이벤트로 기록하며 `import.` 접두 metadata 키를 쓰지 않는다. memory DB의 `surface.meta`는 이 metadata와 별개다.
- 이벤트에 넣지 않는 값:
  - 관측값. terminal의 cwd·복원 명령·scrollback 참조는 surface kind별 저장 자료(자료 참조가 가리키는 payload)로 저장·닫기 시점에 캡처한다.
    cwd가 바뀔 때마다 commit하지 않으며 replay가 과거 cwd를 명령으로 다시 실행하지 않는다.
  - 사용자 선택은 [ADR-0059](0059-id-targets-and-view-owned-selection.md)에 따라 View의 NavigationState가 소유한다. 구조 삭제 뒤의 선택 보정은 View에 적용한다.
  - `focus_second`는 적용기가 기존 hint 규칙대로 채운다.
  - `osc_title`과 표시 이름 같은 Terminal 파생값은 적용 뒤 기존 계산으로 다시 만든다.

### 전환 절차

아래는 전환 단계의 결정이다. 현재 제품 경로와 저장 원본은 다음 절에 별도로 적는다.

- shadow 기간에는 기존 경로가 원본이다. 같은 명령을 CoreState와 JournalModel에 모두 적용하고 두 결과의 구조 digest를 비교한다.
  이 기간의 journal 기록은 비교에만 쓰고 복원에는 쓰지 않는다. digest가 다르면 그 범위를 활성화하지 않는다.
- 활성화는 엔진 단위로 차례로 한다. 엔진 하나가 journal의 stream 하나다([ADR-0063](0063-event-store-storage-fencing-and-effect-states.md)의 ID 예약 절).
  새로 만든 엔진을 먼저 켜고, 레이아웃 슬롯에서 복원한 엔진은 importer의 이관 marker를 확정한 뒤 켠다.
  mirror 구조가 전용 필드로 옮겨지기 전에는 mirror workspace를 가진 엔진을 켜지 않는다.
- 활성화 조건은 0055와 같다. 해당 엔진의 범위 안 writer가 모두 CommandExecutor로 합류하고, 옛 writer와 새 writer가 같은 대상을 섞어 쓰지 않아야 한다.

### 현재 연결 상태

App의 데이터 홈 worker가 엔진별 continuation을 연결하고, worker의 순수 구조 모델을 초기 논리 projection과 확정 batch 적용에 쓴다. App은 별도 가변 JournalModel 원본을 유지하지 않는다.
생성·변환의 단계별 확정 순서, 표시면 자료 참조와 복원 우선순위, legacy slot import, View checkpoint 선택은 [이벤트 저장소](../architecture/event-store.md#구조-journal의-app-연결)에 있다.
source 경계가 존재한다는 사실을 모든 writer·장애 복구 시나리오의 검증 완료로 해석하지 않는다.

### 실행 인스턴스 분리 뒤 모델 재검토

CoreState의 leaf를 SurfaceDescriptor로 바꾸고 사용자 선택을 View로 옮긴 뒤 두 표현을 다시 대조했다. canonical 모델은 저장 revision·ID map·불변 자료 참조·operation을 소유한다. 읽기 projection은 기존 PaneNode/SurfaceLayout과 process-local SplitNodeId를 유지하여 변경되지 않은 노드의 View hint 연결과 geometry 대여를 보존한다. 순수 적용기와 canonical 비교도 같은 tasty-core에 모았으며, 별도 tasty-domain 패키지를 남기지 않는다.

현재는 읽기 projection을 유지하되 독립 decide/ID 발급/로컬 writer를 허용하지 않는다. 노드 identity를 보존하는 단일 표현이 기존 대여 API를 대체하거나, 두 표현의 유지 비용이 반복 결함으로 드러나면 통합을 다시 판단한다. 이 판단은 성능 개선을 실측했다는 뜻이 아니다. 일반 caller와 View는 읽기·명령 포트로 분리하며, 최종 통합 및 실행 검증의 상태는 별도로 기록해야 한다.

## Consequences

기존 JournalModel·`evolve`·importer·CommandExecutor를 그대로 쓴다. CoreState를 먼저 순수 모델로 바꾸지 않아도 전환을 시작할 수 있다.
IPC 응답 형식은 CoreState에서 계속 만들므로 바뀌지 않는다. 선택이 이벤트 밖에 있어 replay가 사용자 포커스를 다시 실행하지 않는다.

이벤트를 CoreState에 적용하는 대응 코드를 따로 유지해야 한다. 적용기 결함은 digest 비교로만 드러난다.
두 모델 유지 비용 판단도 계속 해야 한다(재검토 조건).
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
- [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md) — JournalModel·codec·ID 예약이 놓인 `tasty-core`의 경계
- 현재 구현: `crates/tasty-core/src/{model,state,projection}.rs`, `crates/tasty-model/src/{workspace,tab}.rs`, `src/runtime/command_executor.rs`, `src/runtime/journal_product/view_record.rs`, `src/core/layout_persistence/import.rs`, `src/app/journal/{creation,resource_cleanup,retirement}.rs`.
