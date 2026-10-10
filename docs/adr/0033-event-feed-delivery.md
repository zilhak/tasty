# ADR-0033: 이벤트 피드는 짧게 보관하고 소비자가 읽은 위치를 관리한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: plugins, runtime
- **Group**: plugins

## Context

실시간 구독만 있으면 늦게 접속한 소비자는 지나간 사건과 그 유실을 알 수 없다.
그렇다고 소비자마다 호스트에 읽은 위치와 수신 확인 상태를 두면 느리거나 사라진 소비자의 관리가 서버에 남는다.
필요한 것은 짧은 메모리 보존과 유실을 드러내는 조회 계약이다.

## Decision

debug와 release는 같은 메모리 ring을 사용한다.
각 사건에 증가하는 offset을 부여하고 오래된 사건을 지워도 번호를 재사용하지 않는다.
보존은 개수와 JSON 직렬화 바이트 제한을 함께 적용한다. 최신 사건 하나는 읽을 기회를 주기 위해 크기와 무관하게 남긴다.
디스크 journal과 재전송 보장은 제공하지 않는다.

events.fetch는 소비자가 가져온 위치로 읽는 Local 전용 Read다.
서버는 소비자 cursor를 보관하거나 전진시키지 않는다. Plugin은 기존 subscription을 쓴다.
보존 밖에는 truncated·skipped, 끝보다 큰 위치에는 ahead_of_stream·stream_end를 답한다.
기존 소비자와의 호환을 위해 미래 위치도 요청한 대기 시간과 next_offset 의미를 유지한다.

소비자는 offset과 epoch를 함께 보존한다.
CLI follow는 연결마다 처음에 즉시 조회하고 세대 변경이나 미래 위치를 알린 뒤 처음부터 다시 읽는다.
자동 재연결은 선택 사항이며 사건은 stdout, 유실·연결 알림은 stderr로 나눈다.

agent 사건은 이미 모든 경로가 거치는 task 종결과 barrier 닫힘 지점에서 발행한다.
비종결 전이나 조회 때 평가하는 lease 만료를 완전한 사건 흐름으로 약속하지 않는다.
v2 task 의 단계(phase) 변화도 사건으로 내보내지 않는다(아래 "v2 task 의 단계 변화").
실패 상세·명령 출력은 event payload에 넣지 않고 해당 조회 API로 읽는다.
Experimental 등급은 호환 경고이며 추가 구독 승인 조건은 아니다.

plugin 재발행은 미응답 event.dispatch가 있는 동안 호스트가 hop 하한을 올린다.
판정은 publish 도착 시점에 하며 plugin은 dispatch에 응답해야 한다.
이는 callback 안의 반응을 제한하는 규칙이고 모든 비동기 루프의 완전한 방지는 아니다.

### v2 task 의 단계 변화

[v2 task](0067-typed-task-contracts-live-in-a-separate-record-namespace.md) 의 세부 단계(`executing`·`postprocessing`·`retry_wait`·`awaiting_input`, Ready 의 `waiting_previous_attempt`)는 `task_get`·`task_list` 조회로만 보인다. 단계가 바뀔 때 이벤트 피드나 플러그인 이벤트를 발행하지 않는다. task 사건은 위와 같이 종결(`agent.task_finished`)과 barrier 닫힘뿐이다.

- 단계는 저장된 상태가 아니라 조회 때 계산하는 투영이다. 후처리 단계는 회차의 후처리 진행에서, `awaiting_input` 은 agent 회차의 세션 연결에서, `waiting_previous_attempt` 는 handle 키가 남았는지에서 읽는다. 변화를 사건으로 내려면 이 값을 바꾸는 모든 쓰기 경로(러너 dispatch, 후처리 실행, agent 턴 보고, handle 정리)에 발행 지점을 두거나 tick 마다 전후를 비교해야 한다.
- 새 사건 종류는 번들 플러그인이 공유하는 프로토콜 크레이트에 들어가므로 모든 번들 플러그인의 버전을 올려야 한다.
- 진행 신호가 필요하면 DAG 에 신호를 보내는 task 를 끼운다. 예를 들어 앞 task 뒤에 `tasty notify` 나 webhook 호출(`curl`)을 실행하는 `run` task 를 둔다. 그 task 는 앞 task 가 끝난 뒤 실행되므로 단계 경계의 신호가 된다. 실행 중 단계 자체를 지켜봐야 하면 `task_get` 을 조회한다.

## Consequences

여러 소비자가 서로의 읽기 위치를 바꾸지 않고 유실을 확인할 수 있다.
재시작하면 기록을 잃으며 epoch 없는 재부착은 옛 offset이 새 범위 안에 들어간 경우를 구별하지 못한다.
같은 요청을 반복해도 서버의 읽기 위치를 바꾸지는 않지만 사건 생성·보존 만료 때문에 답은 달라질 수 있다.

최신 단일 대형 사건과 JSON 메모리 표현 때문에 바이트 제한을 RSS 상한으로 볼 수 없다.
대기 요청 하나는 스레드를 차지하며 개별 timeout이 동시 요청 수까지 제한하지는 않는다.
먼저 응답한 뒤 발행하는 plugin은 hop 하한을 벗어나며 무관한 publish가 하한을 적용받을 수도 있다.
피드는 사용자 키·마우스 원문을 재생하거나 화면 상태를 복원하는 통로로 쓰지 않는다.

## Alternatives Considered

- 조용히 보존 시작점부터 주면 소비자는 유실을 알아채지 못한다.
- 서버가 읽기 위치를 관리하면 소비자 등록·해제와 상태 회수도 필요하다.
- 미래 위치에 즉답하면 그 표지를 모르는 client가 빠른 재시도 루프를 만들 수 있다.
- 개수만 제한하면 큰 payload가 호스트 메모리를 오래 점유한다.
- 시간 창이나 trace 자기 신고만으로 재발행을 제한하면 오탐이나 우회가 남는다.
- Experimental opt-in을 뒤늦게 강제하면 현재 구독자 동작이 달라진다.
- 단계 변화를 사건(`agent.task_phase_changed` 같은 새 종류, 또는 단계를 싣는 범용 갱신 사건)으로 발행: 단계를 바꾸는 쓰기 경로마다 발행 지점이 필요하고 번들 플러그인 전부의 버전이 바뀐다. 신호가 필요한 DAG 는 신호 task 로 같은 일을 할 수 있어 기각했다.

## Reconsideration Triggers

실제 skipped 비율과 큰 사건 분포가 보존 요구에 맞지 않으면 상한을 조정한다.
영속 journal·낮은 지연 push·서버측 epoch 검사가 필요하면 기존 조회 의미를 보존하며 비교한다.
비종결 전이가 공통 처리 지점을 갖거나 lease 만료가 명시 전이가 되면 사건 범위를 다시 본다.
단계가 레코드의 저장 필드가 되어 한 쓰기 지점에서 바뀌게 되거나, 신호 task 로는 표현할 수 없는 요구(실행 중 단계 진입을 바로 알아야 하는 소비자)가 실제로 나오면 단계 사건을 다시 검토한다.
SDK 응답 순서와 pump 순서가 바뀌거나 비동기 재발행 루프가 발생하면 hop 정책을 재검토한다.

## References

- [Event Bus 계약과 카탈로그](../reference/event-catalog.md)
- [플러그인 이벤트 사용](../dev-guide/plugin-development.md)
- [터미널 출력의 위치 조회](0034-output-cursor-contract.md)
