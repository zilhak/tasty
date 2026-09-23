# ADR-0617: 구조 변경은 ID를 기준으로 하고 사용자 포커스를 보존한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: workspace, focus, routing
- **Group**: terminal

## Context

사용자와 여러 에이전트가 같은 창과 워크스페이스를 동시에 사용한다. 목록의 위치가 바뀌어도 명령의 대상과 사용자가 보던 항목은 구별되어야 한다.

## Decision

현재 활성 워크스페이스의 내부 표현은 전체 workspace 배열의 인덱스를 유지한다. 카테고리 안의 위치는 사이드바·단축키 입력 단계에서 전체 인덱스로 변환한다. 카테고리 변경만으로 workspace의 물리 순서와 사용자 활성 상태를 바꾸지 않는다.

레이아웃은 engine마다 별도 슬롯 파일에 저장한다. 슬롯 점유 여부는 살아 있는 engine의 layout_slot에서 계산하며 별도 점유 파일이나 registry를 두지 않는다. parked engine도 계속 슬롯을 사용한다. 새 창은 비어 있는 기존 슬롯 중 가장 낮은 번호를 복원하고 모두 사용 중이면 새 번호를 만든다. 부팅에서는 첫 창 하나만 복원한다.

surface ID와 headless PTY ID는 같은 store를 쓰므로 범위가 겹치지 않게 검사한다. Surface는 1 이상 PTY_ID_BASE 미만만 허용한다. 명령 인덱싱·IPC 입력·복원 시 카운터 초기화가 같은 is_surface_id_space 판정을 사용한다. PTY ID를 surface scope로 저장하지 않는다.

항목을 삭제해 앞쪽 인덱스가 바뀌어도 활성 workspace·tab은 같은 ID의 대상을 계속 가리킨다. 사용자가 보던 대상 자체가 사라진 경우에만 다른 대상으로 옮긴다. 이 보정은 사용자·에이전트·원격 요청 모두에 적용한다. pane도 닫힌 pane이 포커스였을 때만 다시 선택한다. 계층별 공용 제거 helper를 사용하고, 이미 지워진 workspace의 위치와 ID는 하나의 Option<(index, id)> 이벤트 값으로 함께 전달한다.

에이전트 workspace.close는 마지막 workspace, mirror workspace, hard 점유 surface가 포함된 workspace, 호출자 자신의 작업이 포함된 대상을 거절한다. 창 종료와 attach 해제는 각 전용 API를 사용한다. headless에는 window.close가 없으므로 마지막 workspace를 닫을 수 없다. 닫기는 되돌릴 수 없음을 help와 사용자 가이드에서 알리며 별도 force 확인을 요구하지 않는다.

category_last_active에는 workspace ID를 저장한다. 카테고리로 돌아갈 때 ID를 찾아 현재 소속을 확인하고 없으면 그 카테고리의 첫 workspace를 고른다. active_workspace와 active_tab은 인덱스를 유지하며, 이동은 active_index_after_move 및 공용 적용 helper를 사용한다. GUI와 headless cascade 모두 같은 보정을 수행한다.

대상을 지정하지 않는 창 소유 자원을 조회하는 목록은 모든 main·parked engine의 결과를 합친다. 메서드 이름에 list가 있는지, params가 있는지만으로 분류하지 않는다. 필터는 대상 지정과 다르다. tree도 workspace.list와 같은 workspace 집합을 반환한다.

합산 전 ID가 창을 넘어 유일해야 한다. hook·global hook·notification ID는 공유 카운터를 사용하며 approval_store는 창 생성 때 같은 Arc를 공유한다. 알림의 전역 목록은 생성 ID 역순 50개를 반환하고 창별 보존·병합·읽음 상태와 UI는 창마다 유지한다.

명시 대상 없이 라우팅되는 메서드는 저장소가 전역인지, 목록 합산인지, 새 대상 생성인지, 다른 단계에서 대상을 찾는지, 아직 해결되지 않은 문제인지 사유를 남긴다. system.info처럼 창별 관측을 반환하면 소유 ID와 scope를 함께 보여준다.

새 창은 WindowRequestOrigin으로 사용자와 에이전트를 구분한다. 사용자 요청은 새 창을 선택한다. 에이전트 요청은 기존 focused_view_id를 유지하고, 기존 main 창이 없을 때만 새 창을 기본 대상으로 삼는다. 에이전트 창은 비활성·숨김 상태로 만들고 OS가 허용하는 방식으로 사용자의 창 뒤에 표시한다. 사용자가 직접 창을 선택한 뒤에는 정상 Focused 이벤트를 따른다.

새 비터미널 탭의 선택 여부도 종류 대신 사용자 origin에서 정한다. CreateTab의 activate 값을 모든 호출자가 전달하며 에이전트 요청은 기존 탭을 보존한다. 도메인 CreateTab의 terminal 분기는 background로 유지한다. 사용자의 새 terminal 탭은 별도 AppState::add_tab 경로에서 선택한다.

요청이 대상을 지정했다면 기록의 소속도 그 대상에서 찾는다. approval.request는 명시 workspace_id, 지정 surface의 workspace, 활성 workspace 순서로 정한다. telemetry는 이미 workspace_id를 받으므로 명시를 권장하되 생략 시 활성 기본값을 유지한다. 활성 상태 조회와 라우팅 전 audit 태그도 유지한다.

## Consequences

기존 이동·닫기·순회 처리를 재사용한다. 카테고리별 보조 상태는 별도로 관리해야 하며 로컬 인덱스와 전역 인덱스 변환은 동일한 workspace 순서를 따라야 한다.

슬롯 파일은 임시 파일을 쓴 뒤 rename한다. restore_layout을 켜면 실제 engine 종료 때 저장하고, 끄면 삭제한다. 같은 TASTY_HOME을 여러 프로세스가 공유하는 구성은 지원하지 않는다. scrollback 정리는 모든 슬롯의 참조를 모아 수행하고 읽지 못한 슬롯이 있으면 그 부팅에서는 삭제하지 않는다.

복원 시 PTY 범위를 침범한 오래된 surface scope는 오류를 기록하고 삭제한다. 복원하지 않는 headless·설정 비활성 실행은 이 정리를 하지 않지만 카운터도 오염된 scope에서 시작하지 않는다. 복원할 명령은 layout에 저장되어 있고 세션 메타는 다시 생성되므로 현재는 재키잉을 하지 않는다.

에이전트가 닫은 항목과 scrollback은 사용자 복원 기록에 넣지 않는다. 점유된 일부만 남기는 부분 workspace.close도 제공하지 않는다. category ID 조회는 사용자의 전환 시점에만 선형 탐색하므로 렌더 루프 비용은 늘지 않는다.

목록 분류 명부는 기존 항목의 누락과 이유를 검토하는 수단이며 새 종류의 목록을 자동 발견하지 못한다. 저장소의 공유 여부는 생성자뿐 아니라 창 생성 시 필드 교체도 함께 읽어 판단한다. 집계한 active는 각 창의 활성 상태이므로 여러 개가 true일 수 있다.

OS의 실제 focus·쌓임 순서는 플랫폼이 결정한다. X11에서는 초기 focus 금지와 restack 요청을 보내고, Wayland에서는 활성화를 요청하지 않는다. 새 창을 만든 뒤 ID 없는 명령이 새 창을 가리킨다고 가정하면 안 된다.

새 비터미널 탭은 사용자가 선택하기 전까지 렌더되지 않는다. 생성·닫기만 하는 release soak는 렌더 저장소의 수명까지 검증하지 못한다.

호스트가 자동 발행한 권한 격상·누적 비용 승인·알림은 활성 workspace를 사용할 수 있다. 누적 비용은 여러 workspace의 합이어서 마지막 이벤트 하나로 귀속할 수 없다. 다만 영속 승인 기록이 활성 workspace에 남는 한계는 해결되지 않았다. 번들 plugin의 workspace_id 없는 telemetry도 같은 기본값을 사용한다.

## Alternatives Considered

활성 상태 전체를 category/local-index 튜플로 바꾸면 영속화·닫기·이동·드래그 처리가 함께 달라진다. workspace 배열을 카테고리별로 나누면 전역 조회와 surface 소유자 탐색이 복잡해진다.

슬롯을 한 파일의 배열로 합치면 동시 flush의 read-modify-write 경합을 해결할 잠금이 필요하다. 점유를 디스크에 쓰면 크래시 뒤 stale 판정이 필요하고, 별도 메모리 registry도 생성·닫기·park 모든 경로에서 갱신해야 한다. 모든 슬롯을 부팅 때 열면 사용자가 원하지 않는 창과 PTY까지 생성한다.

ID 오염을 그대로 두면 두 store 항목을 덮어쓸 수 있으며 범위 검사와 양립하지 않는다. 새 타입으로 store 전체를 바꾸는 안은 세 번째 ID 공간이나 범위 고갈 때 검토한다.

활성 포인터 전체를 ID로 바꾸는 안은 저장 형식과 UI 전반을 함께 바꿔야 하므로 현재 인덱스를 유지한다. 에이전트 삭제만 보정하면 사용자 메뉴의 같은 결함이 남는다. 매 호출 전 ID를 따로 저장하는 안도 모든 삭제 경로에 준비 코드를 넣어야 하므로 제거 위치를 이벤트로 보내는 방식을 사용한다.

마지막 workspace를 닫으면서 창까지 닫거나 새 workspace를 만드는 방법은 요청하지 않은 동작이다. mirror를 일반 close로 지우면 attach 자원 정리를 우회한다. 임의 개수부터 force를 요구할 근거도 없다. 카테고리 재정렬 때 복귀 기록을 지우면 사용자가 보던 대상을 잊고, 인덱스의 소속 검사만 강화해도 같은 카테고리 안의 오선택은 막지 못한다.

이름 규칙이나 줄 단위 grep만으로 목록을 찾으면 tree, 필터 params, helper·serde를 통한 접근을 놓친다. 반대로 모든 CoreState 컬렉션을 목록으로 보면 pending queue까지 포함한다. 자동 추론 대신 이유가 있는 명부와 실제 다중 창 조회를 함께 사용한다. 이름 없는 fallback 목록은 ID 키 검사와 질문이 달라 별도로 유지한다.

키 focus만 유지해도 창이 앞에 나타나면 사용자의 내용을 가린다. OS별 비활성 표시 방법을 사용하되 실패 시 경고를 남기고 기본 표시로 복구한다. tab 생성 뒤 호출자가 active_tab을 되돌리는 방식은 누락하기 쉬우므로 도메인 입력으로 선택 여부를 전달한다.

기록 대상 생략을 전부 거절하면 기존 plugin 호출이 깨진다. 호출자 환경변수의 surface는 요청 대상이 아니며 IPC에도 같은 정보가 없다. 라우터가 이미 없는 surface를 거절하므로 같은 검사를 handler에 중복하지 않는다.

## Reconsideration Triggers

카테고리별 상태가 늘어 변환 계층이 더 복잡해지거나 다중 카테고리 동시 표시 등으로 평면 순서가 맞지 않으면 저장 구조를 다시 검토한다.

슬롯 파일 누적, 창 위치·크기 저장, 여러 프로세스의 홈 공유, 런타임 scrollback 정리가 필요해지면 슬롯 정책을 검토한다. ID 범위 고갈, 제3의 ID 종류, 재시작을 넘어 보존할 surface 메타가 생기거나 복원 없는 실행에서도 정리가 필요해지면 범위와 이행 방법을 재검토한다.

새 삭제 경로에서 보정을 누락하는 문제가 반복되면 활성 포인터를 ID로 바꾼다. 다중 항목 삭제·표시와 저장 순서의 분리·활성 상태 소유 단위 변경 시에는 보정 helper의 입력과 범위를 다시 정한다.

mirror를 전용 teardown으로 닫는 IPC 요구, 규모별 확인의 공통 정책, 에이전트 전용 복원 기록, hard 점유 정의 변경, window.close 정책 변경 시 workspace.close를 함께 검토한다. category ID 탐색이 느려질 규모가 되면 조회 맵을 고려하며 다중 재정렬은 단일 from/to 보정을 일반화해야 한다.

ID가 창별로 중복되거나 여러 active 값이 소비자에 문제를 만들면 집계 형식을 함께 수정한다. serde 대상 키를 분석할 수 있게 되거나 라우팅 규칙이 바뀌면 예외 명부를 줄인다. 창 생성 경로와 실제 핸들러가 명부의 이유에 맞는지는 새 메서드 추가 시 검토한다.

winit을 올릴 때 OS별 show와 active 처리, X11 확장 API를 대조한다. 창이 focus를 가져가거나 위에 뜨는 문제, 반대로 사용자가 골라도 활성화되지 않는 문제는 해당 WM에서 active·stacking·user-time 속성을 측정한다. 새 창·탭 생성 경로는 origin을 잃지 않아야 한다. 사용자 terminal 생성이 CreateTab으로 옮겨지면 terminal의 background 고정을 재검토한다.

telemetry가 workspace를 항상 지정하거나 비용 상한이 workspace별로 나뉘면 기본 귀속 정책을 검토한다. 자동 승인에 실제 대상 surface가 추가되면 활성 workspace 대신 그 소속을 사용할 수 있다.

## References

- [포커스 정책](../design/policies/focus.md)
- [워크스페이스 카테고리](../features/workspace-category/index.md)
