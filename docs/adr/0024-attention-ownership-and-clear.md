# ADR-0024: 주의 환기 상태는 surface 소유자가 관리한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: attention, notifications, ownership
- **Group**: terminal

## Context

작업 완료와 입력 요청은 테두리·탭 제목·workspace 배지로 알린다. 알림 패널에 항목을 만들지 않는 이벤트도 같은 표시를 사용할 수 있어야 하며 mirror가 원본과 따로 상태를 만들면 안 된다.

## Decision

AttentionStore가 surface별 kind와 발생 시각을 보관한다. Completion과 NeedsInput처럼 종류별 효과는 effects_of에서 정한다. NotificationStore와 별도로 두고 패널 항목이 필요한 발생원이 직접 알림을 만든다. 화면의 Highlight는 이 Attention 상태를 표시한다.

mirror attention의 원본은 서버 push다. 로컬 raise_attention은 mirror 대상이면 상태를 만들지 않는다. 서버 push는 mirror 전용 setter로 적용한다. mirror completion을 서버로 다시 보내지 않는다. 동일한 OSC 출력은 서버도 이미 처리하기 때문이다.

실제 렌더에서 사용자가 surface를 보거나 관련 알림을 읽으면 attention을 해제한다. mirror에서 실제 레코드를 지운 순간만 ClientAttentionClear를 큐에 넣고 server는 해당 workspace holder인지 검사한 뒤 지운다. 반복 프레임마다 focus를 보내지 않는다. 서버 push와 teardown은 별도 setter·forget 경로를 사용해 해제 메시지 반복을 만들지 않는다.

hard 점유 중 서버 로컬 GUI는 clear_attention_local에서 해제를 건너뛴다. 알림 패널의 읽음 상태는 그대로 바꿀 수 있다. holder가 확인한 해제는 검증 후 기본 clear_attention을 직접 사용한다. 기본 함수에 hard 검사를 넣어 holder까지 막지 않는다. soft 점유는 로컬 해제를 허용한다.

IPC·CLI는 surface.attention.get/clear를 제공한다. clear는 hard 점유와 mirror를 명시적으로 거절하고 get은 허용한다. 선택 kind 필터는 정확히 같은 kind만 지우며 모르는 값은 거절한다. completion의 기존 미상 kind 기본값과 구분한다. 상태 적용은 라우팅된 owner engine의 핸들러에서 수행해 headless에서도 동작하게 한다.

## Consequences

같은 attention을 여러 발생원이 사용할 수 있다. mirror raise 억제는 attention만 대상으로 하며 같은 흐름의 panel 항목·toast·hook event까지 억제하지 않는다. 현재 mirror completion은 성공을 반환하고 attention 변경은 하지 않으므로 호출자가 억제 여부를 응답만으로 알 수 없다.

mirror에서 이미 보고 있는 surface에 새 attention이 오면 다음 렌더에서 지워 서버로 확인을 전달한다. 한 공유 레코드이므로 누가 확인했는지 따로 보존하지 않는다. 전송 실패한 clear는 재시도하지 않는다. 오래된 확인이 나중에 도착해 새 신호를 지우는 것을 피한다.

surface 단위 raw attach는 workspace holder 기반 해제 경로와 GUI 확인을 사용하지 않아 점유 중 해제 수단이 없다. detach 뒤에는 서버 로컬 확인이 다시 가능하다. GUI 후처리는 redraw를 담당하며 필터 불일치인 clear도 한 번 다시 그릴 수 있다.

## Alternatives Considered

Notification에 kind를 넣으면 panel 항목 없는 완료 이벤트를 표현하기 어렵다. kind별 별도 집합은 소비자가 우선순위를 반복해서 계산하게 한다. 발생원마다 mirror 검사를 넣으면 새 경로를 빠뜨리기 쉽다. 표시만 숨기고 store를 유지하면 count와 IPC에서 다른 값이 나타난다.

포커스 경로에서만 forward하면 알림 읽음 해제를 놓친다. 입력·스크롤을 확인으로 간주하면 로컬과 mirror 규칙이 달라진다. hard 점유 중 알림 읽기나 focus 자체를 막을 이유는 없다. 이들은 서버 사용자 로컬 상태이고 attention만 holder와 공유한다.

headless에서 소비되지 않는 GUI intent만 enqueue하면 상태가 바뀌지 않는다. 전체 도메인 intent 실행을 한꺼번에 켜는 대신 attention 적용을 핸들러에서 보장한다. kind 없는 무조건 clear만 제공하면 완료 확인이 나중에 발생한 입력 요청까지 지울 수 있다.

## Reconsideration Triggers

종류별 효과 조합이 복잡해지거나 Attention/Highlight 이름이 혼란을 만들면 모델과 명칭을 검토한다. mirror에서만 관측 가능한 신호, 로컬 전용 표시, workspace 단위 상태가 필요하면 서버 소유 규칙을 다시 정한다. completion 억제를 오류나 suppressed 필드로 알리는 API 변경은 별도 호환 검토가 필요하다.

관측자별 확인 상태나 점유 없는 mirror, 서버 사용자의 대리 확인, 새 해제 이벤트가 필요하면 권한을 다시 정한다. headless가 같은 intent를 실행하게 되면 직접 적용과 후처리 중복을 제거할 수 있는지 검토한다. mirror IPC clear를 허용할 때는 핸들러와 후처리 사이 새 push를 지우는 문제가 없는지도 본다. attention 해제와 알림 읽기를 합칠 API가 필요해도 저장소의 의미는 구분한다.

## References

- [주의 환기 기능](../features/surface-highlight/index.md)
- [attach 구현](../dev-guide/attach-behavior.md)
