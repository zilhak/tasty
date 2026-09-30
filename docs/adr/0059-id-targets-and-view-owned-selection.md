# ADR-0059: 구조 명령은 ID로 대상을 정하고 사용자 선택은 View가 소유한다

- **Status**: Accepted — 구현 상태: 단계적 이행 중. 구조 선택과 카테고리 접힘은 View의 ID 기반 navigation에 있으며 headless는 별도 명령 기본 문맥을 사용한다. 터미널 viewport 분리와 저널 원본 전환은 이행 중이며 레이아웃 슬롯 파일은 복원 원본으로 남아 있다
- **Date**: 2026-09-30
- **Tags**: workspace, focus, routing, identity, layout
- **Group**: terminal

## Context

사용자와 여러 에이전트가 같은 창과 workspace를 동시에 사용한다. 목록의 위치가 바뀌어도 명령의 대상과 사용자가 보던 항목은 구별되어야 한다.

[ADR-0017](0017-workspace-identity-and-focus.md)은 이 요구를 당시 구조 안에서 풀었다. 사용자 선택(활성 workspace·tab 인덱스, 카테고리 복귀 기록)을
도메인 트리와 같은 `CoreState`에 두고, 삭제 때 공용 제거 함수가 선택을 보정하며, 레이아웃은 엔진별 슬롯 파일을 원본으로 저장했다.
선택을 도메인과 분리하면 저장 형식과 UI 전반이 함께 바뀐다는 비용 때문에 이 배치를 유지했다.

[ADR-0054](0054-app-core-view-layers-and-state-ownership.md)와 [ADR-0055](0055-structural-domain-event-sourcing.md)에서는 전제가 다르다.
재생 가능한 도메인 모델에 사용자 선택이 있으면 replay나 지연 완료가 과거 선택을 다시 실행한다.
레이아웃 슬롯 파일과 이벤트 저널을 둘 다 원본으로 두면 두 원본이 갈라진다. 선택은 View, 구조는 기록이 소유해야 한다.

## Decision

### 대상과 선택의 소유

- 구조 명령은 대상을 ID로 지정한다. 대상 생략은 기존 호환 규칙에 따라 진입 계층이 해소하고, 해소한 ID를 명령에 고정한다.
- 활성 workspace·tab·pane, 카테고리 복귀 기록, 카테고리 접힘, viewport는 `ViewState`가 소유한다. `CoreState`에는 사용자 선택을 두지 않는다.
  분할 트리의 `focus_second`는 선택의 원본이 아니라 생성·복원·wire 왕복에 쓰는 호환 hint로만 다룬다.
- OS가 알려 준 창 focus와 App의 기본 IPC 라우팅 문맥(대상 없는 요청이 향하는 main 창)은 다른 값이다. 라우팅 문맥과 전역 modal은 `AppState` 한 곳에 둔다.
- 에이전트 요청은 사용자 선택·스크롤·닫은 항목 기록을 바꾸지 않는다. 예외는 사용자가 보던 대상 자체가 삭제됐을 때의 필수 보정이다.
  보정은 View가 확정된 삭제 사실을 소비해서 한다. 도메인이 View의 선택을 직접 고치지 않는다.

### 유지하는 제품 규칙

아래 규칙은 새 배치에서도 그대로 유지한다. 현재 동작의 세부는 [포커스 정책](../design/policies/focus.md)과 각 기능 문서가 설명한다.

- 앞쪽 항목이 지워져도 활성 workspace·tab은 같은 ID의 대상을 계속 가리키고, 보던 대상이 사라졌을 때만 다른 대상으로 옮긴다.
  pane도 닫힌 pane이 포커스였을 때만 다시 선택한다. 이 보정은 사용자·에이전트·원격 요청 모두에 적용하며 GUI와 헤드리스가 같은 보정을 수행한다.
  이미 지워진 workspace의 위치와 ID는 한 삭제 사실에 함께 싣는다.
- 활성 workspace는 전체 workspace 순서 기준으로 표현하고, 카테고리 안의 위치는 사이드바·단축키 입력 단계에서 변환한다.
  카테고리 변경만으로 workspace의 물리 순서와 사용자 활성 상태를 바꾸지 않는다.
  카테고리 복귀 기록은 workspace ID로 저장하고, 돌아갈 때 현재 소속을 확인해 없으면 그 카테고리의 첫 workspace를 고른다.
- 에이전트의 `workspace.close`는 마지막 workspace, mirror workspace, hard 점유 surface를 포함한 workspace, 호출자 자신의 작업이 포함된 대상을 거절한다.
  이 거절은 도메인 불변식이며 진입점 권한 검사와 별개다. 창 종료와 attach 해제는 각 전용 API를 사용한다.
  헤드리스에는 `window.close`가 없으므로 마지막 workspace를 닫을 수 없다. 닫기는 되돌릴 수 없음을 도움말과 사용자 가이드에서 알리며 별도 force 확인은 요구하지 않는다.
  점유된 일부만 남기는 부분 close는 제공하지 않으며, 에이전트가 닫은 항목과 scrollback은 사용자 복원 기록에 넣지 않는다.
- 새 창은 요청 출처(`WindowRequestOrigin`)로 사용자와 에이전트를 구분한다. 사용자 요청은 새 창을 라우팅 문맥으로 선택한다.
  에이전트 요청은 기존 라우팅 문맥을 유지하고, 기존 main 창이 없을 때만 새 창을 기본 대상으로 삼는다.
  에이전트 창은 비활성·숨김 상태로 만들고 OS가 허용하는 방식으로 사용자 창 뒤에 표시하며, 사용자가 직접 창을 선택한 뒤에는 정상 Focused 이벤트를 따른다.
  OS의 실제 focus·쌓임 순서는 플랫폼이 결정한다. X11에서는 초기 focus 금지와 restack 요청을 보내고, Wayland에서는 활성화를 요청하지 않는다.
  비활성 표시에 실패하면 경고를 남기고 기본 표시로 복구한다.
- 새 탭의 선택 여부는 종류가 아니라 사용자 요청 여부로 정한다. CreateTab의 activate 값을 모든 호출자가 전달하며 에이전트 요청은 기존 탭을 보존한다.
  도메인 CreateTab의 terminal 분기는 activate와 무관하게 background로 유지하고, 사용자의 새 terminal 탭은 별도 경로(현재 `MainViewState::add_tab`)에서 선택한다.
  에이전트가 만든 새 비터미널 탭은 사용자가 선택하기 전까지 렌더되지 않는다.
- 대상을 지정하지 않는 창 소유 자원 목록은 살아 있는 모든 엔진(창이 있는 엔진과 parked 엔진)의 결과를 합친다. 메서드 이름에 list가 있는지나 params 유무로 분류하지 않고,
  필터는 대상 지정과 다르다. tree도 `workspace.list`와 같은 workspace 집합을 반환한다. 집계한 active는 엔진별 활성 상태이므로 여러 개가 true일 수 있다.
  합산하는 ID는 엔진을 넘어 유일해야 한다. hook·global hook·notification ID는 공유 카운터를 쓰고 approval 저장소는 엔진 생성 때 같은 것을 공유한다.
  알림의 전역 목록은 생성 ID 역순 50개를 반환하고 엔진별 보존·병합·읽음 상태와 UI는 따로 유지한다.
- 명시 대상 없이 라우팅되는 메서드는 저장소가 전역인지, 목록 합산인지, 새 대상 생성인지, 다른 단계에서 대상을 찾는지, 아직 해결되지 않은 문제인지 사유를 명부에 남긴다.
  `system.info`처럼 엔진별 관측을 반환하면 소유 ID와 scope를 함께 보여준다.
- 요청이 대상을 지정했다면 기록의 소속도 그 대상에서 찾는다. `approval.request`는 명시 `workspace_id`, 지정 surface의 workspace, 활성 workspace 순서로 정한다.
  telemetry는 workspace 명시를 권장하되 생략 시 활성 기본값을 유지한다. 대상 정보가 전혀 없는 요청의 활성 workspace 기본값은 호환을 위해 유지한다.
  호스트가 자동 발행한 권한 격상·누적 비용 승인·알림과 번들 plugin의 workspace 없는 telemetry도 활성 workspace를 쓴다.
  누적 비용은 여러 workspace의 합이라 이벤트 하나로 귀속할 수 없으며, 영속 승인 기록이 활성 workspace에 남는 한계는 해결되지 않았다.
- surface ID와 standalone PTY ID는 겹치지 않는 범위를 쓴다. surface는 1 이상 PTY ID 기준값 미만이며 명령 해석·IPC 입력·복원 카운터 초기화가 같은 범위 판정을 사용한다.
  PTY ID를 surface scope로 저장하지 않는다. 새 구조에서는 IdentityAllocator가 재사용하지 않는 typed ID를 예약하되 기존 범위와 wire 표현은 유지한다.

### 레이아웃 슬롯

- 슬롯은 View 복원 자료의 단위다. 엔진마다 하나의 슬롯을 쓰고, 점유 여부는 살아 있는 엔진의 슬롯에서 계산하며 별도 점유 파일이나 registry를 두지 않는다.
  parked 엔진도 슬롯을 계속 사용한다. 새 창은 비어 있는 가장 낮은 슬롯을 복원하고, 모두 사용 중이면 새 번호를 만든다. 부팅에서는 첫 창 하나만 복원한다.
- 슬롯 파일은 임시 파일을 쓴 뒤 rename한다. `restore_layout`을 켜면 실제 엔진 종료 때 저장하고, 끄면 삭제한다.
- scrollback 정리는 모든 슬롯의 참조를 모아 수행하고, 읽지 못한 슬롯이 있으면 그 부팅에서는 삭제하지 않는다.
- 복원 시 PTY 범위를 침범한 오래된 surface scope는 오류를 기록하고 삭제한다. 복원하지 않는 헤드리스·설정 비활성 실행은 이 정리를 하지 않지만 카운터도 오염된 scope에서 시작하지 않는다.
  복원할 명령은 레이아웃에 저장되고 세션 메타는 다시 생성되므로 현재는 키를 새로 발급하지 않는다.
- 구조 저널을 활성화하기 전에는 슬롯 파일이 지금처럼 복원 원본이다. 활성화 뒤에는 최초 한 번 가져온 다음 저널이 구조 원본이 되고,
  슬롯 파일은 View 선택 복원 자료와 호환 export로 남는다. 오래된 슬롯 파일을 다시 구조 원본으로 가져오지 않는다.
- 같은 `TASTY_HOME`을 여러 프로세스가 공유하는 구성은 지원하지 않는다.

## Consequences

replay와 지연 완료가 사용자 선택을 다시 실행하지 않는다. 에이전트 요청이 선택을 바꾸는 경로는 View 경계에서만 생길 수 있어 검사하기 쉽다.
기존 사용자에게 보이는 포커스 보존·닫기 거절·창 표시 규칙은 그대로다.

목록 분류 명부는 기존 항목의 누락과 이유를 검토하는 수단이며 새 종류의 목록을 자동 발견하지 못한다. 저장소의 공유 여부는 생성자뿐 아니라 엔진 생성 때의 필드 교체까지 읽어 판단한다.
category ID 조회는 사용자 전환 시점에만 선형 탐색하므로 렌더 루프 비용은 늘지 않는다. 생성과 닫기만 반복하는 release 장시간 테스트는 렌더 저장소의 수명까지 검증하지 못한다.

선택 보정이 도메인 제거 함수에서 View의 삭제 사실 소비로 옮겨지므로, 모든 삭제 이벤트가 View까지 전달되는지 검사해야 한다.
이행 중에는 선택이 `CoreState`에 남아 있는 경로와 View로 옮겨진 경로가 섞인다. 한 범위의 선택을 옮길 때 그 범위의 소비자를 같은 변경에서 전부 옮긴다.

슬롯 파일과 저널이 공존하는 이행 기간에는 어느 쪽이 원본인지 범위별로 명확해야 한다. 저널을 읽지 못하는 옛 바이너리가 새 상태를 덮어쓰지 않도록 소유 검사가 필요하다.
OS의 실제 focus·쌓임 순서는 플랫폼이 결정하므로 새 창을 만든 뒤 대상 없는 명령이 새 창을 가리킨다고 가정하면 안 된다.

## Alternatives Considered

- 선택을 `CoreState`에 둔 채 replay에서만 무시하는 안: 같은 필드가 재생 모델과 사용자 상태를 겸해 경계가 다시 흐려진다.
- 선택 보정을 도메인이 계속 수행하는 안: 도메인이 View 상태를 알아야 하고, 로컬 View가 없는 엔진에서 보정 대상이 없다.
- 슬롯 파일을 계속 구조 원본으로 두고 저널을 보조로 쓰는 안: 두 원본이 갈라졌을 때 판정 기준이 없다.
- 점유를 디스크나 별도 registry에 기록하는 안: crash 뒤 stale 판정이 필요하고 생성·닫기·park 모든 경로에서 갱신해야 한다.
- 에이전트 삭제만 보정하는 안: 사용자 메뉴의 같은 결함이 남는다.
- 대상 생략을 전부 거절하는 안: 기존 plugin·CLI 호출이 깨진다. 호출자 환경변수의 surface는 요청 대상이 아니며 IPC에도 같은 정보가 없다.
  라우터가 이미 없는 surface를 거절하므로 같은 검사를 handler에 중복하지 않는다.
- 슬롯을 한 파일의 배열로 합치는 안: 동시 flush의 read-modify-write 경합을 막을 잠금이 필요하다. 모든 슬롯을 부팅 때 여는 안은 원하지 않는 창과 PTY까지 만든다.
- 활성 상태를 category·로컬 인덱스 쌍으로 바꾸거나 workspace 배열을 카테고리별로 나누는 안: 영속화·닫기·이동·드래그와 전역 조회·surface 소유자 탐색이 함께 복잡해진다.
- 마지막 workspace를 닫으며 창까지 닫거나 새 workspace를 만드는 안: 요청하지 않은 동작이다. mirror를 일반 close로 지우면 attach 자원 정리를 우회한다.
- 이름 규칙이나 줄 단위 grep만으로 목록 메서드를 찾는 안: tree, 필터 params, helper·serde를 통한 접근을 놓친다. 이유가 있는 명부와 실제 다중 엔진 조회를 함께 쓴다.
- 키보드 포커스만 유지하고 창을 앞에 띄우는 안: 새 창이 사용자의 내용을 가린다. tab 생성 뒤 호출자가 선택을 되돌리는 안은 누락하기 쉬워 선택 여부를 입력으로 전달한다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 한 엔진을 여러 로컬 View가 함께 표시하게 되면 선택 소유 단위와 보정 규칙을 다시 정한다.
- 카테고리별 상태가 늘어 변환 계층이 복잡해지거나 다중 카테고리 동시 표시가 필요해지면 선택 표현을 다시 검토한다.
- 슬롯 파일 누적, 창 위치·크기 저장, 여러 프로세스의 홈 공유가 필요해지면 슬롯 정책을 검토한다.
- 제3의 ID 종류가 생기거나 범위가 고갈되면 ID 할당과 wire 표현을 재검토한다.
- 사용자 terminal 생성이 CreateTab으로 옮겨지면 terminal 분기의 background 고정을 재검토한다. 새 창·탭 생성 경로는 요청 출처를 전달해야 한다.
- mirror를 전용 teardown으로 닫는 IPC 요구, 규모별 확인 정책, 에이전트 전용 복원 기록, hard 점유 정의 변경, `window.close` 정책 변경 시 `workspace.close`를 함께 검토한다.
- telemetry가 workspace를 항상 지정하거나 비용 상한이 workspace별로 나뉘면 기본 귀속 정책을 검토한다. 자동 승인에 실제 대상 surface가 생기면 그 소속을 사용한다.
- 엔진별 중복 ID나 여러 active 값이 소비자에 문제를 만들면 집계 형식을 함께 고친다. 라우팅 규칙이 바뀌면 명부의 예외를 줄인다.
- 런타임 scrollback 정리나 재시작을 넘어 보존할 surface 메타가 필요해지면 슬롯·ID 정책을 검토한다.
- category ID 탐색이 느려질 규모가 되면 조회 맵을 고려하며, 다중 재정렬은 단일 from/to 보정을 일반화해야 한다.
- 새 삭제 경로에서 View 보정이 빠지는 문제가 반복되면 삭제 사실 전달 경계를 검사로 고정한다.

### 실행 결과로 확인

- winit을 올릴 때 OS별 show·active 처리와 X11 확장을 대조한다. 창이 focus를 가져가거나 위에 뜨는 문제는 해당 WM에서 active·stacking 속성을 측정한다.

## References

- 대체 대상: [ADR-0017](0017-workspace-identity-and-focus.md)
- [ADR-0054](0054-app-core-view-layers-and-state-ownership.md) · [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0021](0021-occupancy-and-attach-admission.md)
- [포커스 정책](../design/policies/focus.md)
- [워크스페이스 카테고리](../features/workspace-category/index.md)
- [레이아웃 저장](../features/layout-persistence/index.md)
- 현재 구현: `src/core/state.rs`, `src/core/layout_persistence`, `src/adapters/ipc/window_port.rs`.
